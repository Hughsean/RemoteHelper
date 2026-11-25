use std::{
    fs,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock, atomic::Ordering},
    time::Duration,
};

use ::history::{
    Catalog, ClockEvidence, DAY_HEADER_LEN, DayHeader, FileContext, FileKind, Frame, Packet,
    Session, decode_packet,
};
use base64::prelude::*;
use common::{
    Handshake, Request, Response, SampleIdentity,
    crypto::{CryptoSession, generate_ephemeral},
    history_api::{
        DirectoryEntry, EditableHistory, HISTORY_FRAME_LIMIT, HistoryErrorCode, HistoryRequest,
        HistoryResponse, PacketPage, SettingsRevision, TailCursor,
    },
    server_auth::{HANDSHAKE_MAX, HISTORY_VERSION, decode_base64, history_login_message},
    transport::{FrameResult, recv_frame, send_frame},
};
use ed25519_dalek::{Signer, SigningKey};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::timeout,
};
use x25519_dalek::PublicKey;

use super::{Inbox, ReadState, RecorderHandle, RecorderOptions, Sample, StorageState, Store};
use crate::{
    config::{AppConfig, HistoryConfig, WebPanelConfig},
    identity::ServerIdentity,
    state::AppState,
};

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const SESSION: Session = Session {
    session_id: [42; 16],
    boot_id: Some([12; 16]),
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "remotehelper-history-e2e-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn public_key(key: &SigningKey) -> String {
    BASE64_STANDARD.encode(key.verifying_key().to_bytes())
}

fn fixture(path: &Path, key: &SigningKey) -> (Store, AppState, String) {
    let read = Arc::new(RwLock::new(ReadState::new(path.to_owned())));
    let store = Store::open(path.to_owned(), read.clone()).unwrap();
    read.write().unwrap().set_storage(StorageState::Recording);
    // 直接驱动真实存储，使追加和封存可确定，不启动后台采样或等待计时器。
    let recorder = RecorderHandle {
        read,
        inbox: Arc::new(Inbox {
            pending: Mutex::default(),
            ready: Default::default(),
        }),
    };
    let config = AppConfig {
        web_panel: WebPanelConfig {
            enabled: false,
            local_port: 0,
            frpc_exe_path: String::new(),
            frpc_args: vec![],
            authorized_keys: vec![public_key(key)],
            startup_delay_secs: 0,
            max_connections: 1,
            connection_timeout_secs: 60,
            health_check_url: None,
        },
        history: HistoryConfig {
            dir: path.to_owned(),
            ..HistoryConfig::default()
        },
        service: vec![],
    };
    let mut state = AppState::new(config, Some(recorder), b"");
    let identity = ServerIdentity::for_test();
    let pin = identity.fingerprint();
    state.identity = Some(Arc::new(identity));
    (store, state, pin)
}

struct Client {
    socket: TcpStream,
    crypto: CryptoSession,
    transcript: Option<[u8; 32]>,
    buffer: Vec<u8>,
}

impl Client {
    async fn connect(state: AppState, pin: Option<&str>) -> (Self, JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            drop(listener);
            state.active_connections.fetch_add(1, Ordering::Relaxed);
            crate::server::handle_connection(socket, state).await;
        });
        // 使用共享帧编解码而非客户端历史封装，以便验证服务端自身的访问控制。
        let client = timeout(IO_TIMEOUT, async {
            let mut socket = TcpStream::connect(address).await.unwrap();
            let (secret, ephemeral) = generate_ephemeral();
            let offered: &[u16] = if pin.is_some() {
                &[HISTORY_VERSION]
            } else {
                &[]
            };
            let hello = serde_json::to_vec(&Handshake::ClientHello {
                public_key: BASE64_STANDARD.encode(ephemeral.as_bytes()),
                history_versions: offered.to_vec(),
            })
            .unwrap();
            socket.write_u32(hello.len() as u32).await.unwrap();
            socket.write_all(&hello).await.unwrap();
            let len = socket.read_u32().await.unwrap() as usize;
            assert!((1..=HANDSHAKE_MAX).contains(&len));
            let mut reply = vec![0; len];
            socket.read_exact(&mut reply).await.unwrap();
            let Handshake::ServerHello {
                public_key,
                history,
            } = serde_json::from_slice(&reply).unwrap()
            else {
                panic!("应返回 ServerHello");
            };
            let server_key = decode_base64(&public_key).unwrap();
            let transcript = match (pin, history) {
                (Some(pin), Some(proof)) => {
                    let (identity, transcript) = proof
                        .verify(ephemeral.as_bytes(), &server_key, offered)
                        .unwrap();
                    assert_eq!(identity.fingerprint, pin);
                    assert_eq!(identity.history_version, HISTORY_VERSION);
                    Some(transcript)
                }
                (None, None) => None,
                _ => panic!("服务端历史版本协商结果不符"),
            };
            let shared = secret.diffie_hellman(&PublicKey::from(server_key));
            Self {
                socket,
                crypto: CryptoSession::new(shared.to_bytes(), false),
                transcript,
                buffer: vec![0; HISTORY_FRAME_LIMIT],
            }
        })
        .await
        .expect("测试握手超时");
        (client, server)
    }

    async fn request(&mut self, request: Request) -> Response {
        timeout(IO_TIMEOUT, async {
            send_frame(&mut self.socket, &mut self.crypto, &request)
                .await
                .unwrap();
            match recv_frame(&mut self.socket, &mut self.crypto, &mut self.buffer)
                .await
                .unwrap()
            {
                FrameResult::Message(response) => response,
                FrameResult::Invalid(detail) => panic!("响应帧无效：{detail}"),
                FrameResult::Closed => panic!("读取响应前连接已关闭：{request:?}"),
            }
        })
        .await
        .unwrap_or_else(|_| panic!("等待响应超时：{request:?}"))
    }

    async fn login(&mut self, key: &SigningKey, bind_transcript: bool) -> Response {
        let Response::Challenge(nonce) = self.request(Request::GetChallenge).await else {
            panic!("登录需要挑战");
        };
        let nonce: [u8; 32] = decode_base64(&nonce).unwrap();
        let public_key = public_key(key);
        let signature = BASE64_STANDARD.encode(
            match self.transcript.filter(|_| bind_transcript) {
                Some(hash) => key.sign(&history_login_message(&nonce, &hash)),
                None => key.sign(&nonce),
            }
            .to_bytes(),
        );
        let request = if self.transcript.is_some() {
            Request::LoginHistory {
                public_key,
                signature,
            }
        } else {
            Request::Login {
                public_key,
                signature,
            }
        };
        self.request(request).await
    }

    async fn history(&mut self, request: HistoryRequest) -> HistoryResponse {
        match self.request(Request::History(request)).await {
            Response::History(response) => response,
            other => panic!("应返回历史响应，实际返回 {other:?}"),
        }
    }

    async fn close(self, server: JoinHandle<()>) {
        drop(self);
        timeout(IO_TIMEOUT, server)
            .await
            .expect("测试连接未关闭")
            .unwrap();
    }
}

fn assert_error(response: HistoryResponse, expected: HistoryErrorCode) {
    match response {
        HistoryResponse::Error(error) => assert_eq!(error.code, expected, "{error:?}"),
        other => panic!("应返回历史错误 {expected:?}，实际返回 {other:?}"),
    }
}

async fn tail(client: &mut Client, cursor: Option<TailCursor>) -> PacketPage {
    match client.history(HistoryRequest::Tail { cursor }).await {
        HistoryResponse::Tail(page) => page,
        other => panic!("应返回尾部页，实际返回 {other:?}"),
    }
}

fn page_bytes(page: &PacketPage) -> Vec<u8> {
    let bytes = BASE64_STANDARD.decode(&page.bytes).unwrap();
    assert!(page.offset <= page.next_offset && page.next_offset <= page.through);
    assert_eq!(page.next_offset - page.offset, bytes.len() as u64);
    if let Some(cursor) = page.tail_cursor {
        assert_eq!(cursor.offset, page.next_offset);
        assert_eq!(cursor.through, page.through);
    }
    bytes
}

fn sample(seq: u64) -> Sample {
    let time_ms = 1_000 * seq as i64;
    Sample {
        session: SESSION,
        frame: Frame {
            sample_seq: seq,
            time_ms,
            period_ms: 1_000,
            clock: ClockEvidence {
                wall_ms: Some(time_ms),
                tick_ms: Some(time_ms as u64),
                awake_ms: Some(time_ms as u64),
            },
            values: vec![None; Catalog::standard(1).channels.len()],
        },
    }
}

fn decoded_samples(bytes: &[u8], kind: FileKind) -> Vec<(SampleIdentity, i64)> {
    DayHeader::decode(bytes).unwrap();
    let mut context = FileContext::new(kind);
    let mut samples = Vec::new();
    let mut offset = DAY_HEADER_LEN;
    while offset < bytes.len() {
        let (raw, used) = decode_packet(&bytes[offset..]).unwrap();
        let mut push = |session: &Session, frame: Frame| {
            samples.push((
                SampleIdentity {
                    session_id: session.session_id,
                    sample_seq: frame.sample_seq,
                },
                frame.time_ms,
            ));
        };
        match context.decode_at(offset as u64, &raw).unwrap() {
            Packet::Frame(frame) => push(context.current_session().unwrap().1, frame),
            Packet::Block(block) => {
                let session = context.session(block.session_ref).unwrap();
                for frame in block.frames {
                    push(session, frame);
                }
            }
            _ => {}
        }
        offset += used;
    }
    samples
}

#[tokio::test]
async fn signed_history_preserves_sample_identity_across_tail_and_seal() {
    let dir = TempDir::new();
    let key = SigningKey::from_bytes(&[27; 32]);
    let (mut store, state, pin) = fixture(&dir.0, &key);
    let options = RecorderOptions {
        enabled: true,
        seal_interval: Duration::from_secs(300),
        retention_days: 30,
        retention_bytes: 0,
        compression_level: 3,
    };
    let first = sample(1);
    let second = sample(2);
    store.record(&first, &[], &options).unwrap();
    let (mut client, server) = Client::connect(state, Some(&pin)).await;
    assert!(matches!(client.login(&key, true).await, Response::Ok));

    let initial = tail(&mut client, None).await;
    let old_cursor = initial.tail_cursor.unwrap();
    assert_eq!(initial.offset, 0);
    assert_eq!(initial.next_offset, initial.through);
    let first_bytes = page_bytes(&initial);
    assert_eq!(decoded_samples(&first_bytes, FileKind::Wal).len(), 1);

    store.record(&second, &[], &options).unwrap();
    // 旧快照的终点不会随正常追加推进，重新打开尾部才会看到新样本。
    let finished = tail(&mut client, Some(old_cursor)).await;
    assert_eq!(finished.offset, old_cursor.offset);
    assert_eq!(finished.tail_cursor, Some(old_cursor));
    assert!(page_bytes(&finished).is_empty());
    let resume = TailCursor {
        offset: DAY_HEADER_LEN as u64,
        ..old_cursor
    };
    let remaining = tail(&mut client, Some(resume)).await;
    assert_eq!(remaining.offset, resume.offset);
    assert_eq!(remaining.tail_cursor, Some(old_cursor));
    assert_eq!(page_bytes(&remaining), first_bytes[DAY_HEADER_LEN..]);
    let fresh = tail(&mut client, None).await;
    let fresh_cursor = fresh.tail_cursor.unwrap();
    assert_eq!(fresh_cursor.wal_generation, old_cursor.wal_generation);
    assert!(fresh.through > old_cursor.through);
    let fresh_bytes = page_bytes(&fresh);
    assert!(fresh_bytes.starts_with(&first_bytes));
    let expected = [first, second].map(|s| {
        (
            SampleIdentity {
                session_id: s.session.session_id,
                sample_seq: s.frame.sample_seq,
            },
            s.frame.time_ms,
        )
    });
    assert_eq!(decoded_samples(&fresh_bytes, FileKind::Wal), expected);

    store.seal(options.compression_level).unwrap();
    assert_error(
        client
            .history(HistoryRequest::Tail {
                cursor: Some(fresh_cursor),
            })
            .await,
        HistoryErrorCode::TailRestartRequired,
    );
    let restarted = tail(&mut client, None).await;
    assert_ne!(
        restarted.tail_cursor.unwrap().wal_generation,
        fresh_cursor.wal_generation
    );
    assert_eq!(restarted.offset, 0);
    assert_eq!(restarted.next_offset, restarted.through);
    let restarted_bytes = page_bytes(&restarted);
    assert!(decoded_samples(&restarted_bytes, FileKind::Wal).is_empty());

    let HistoryResponse::Summary(summary) = client.history(HistoryRequest::Summary).await else {
        panic!("应返回历史摘要");
    };
    assert!(summary.store_id.is_some());
    assert_eq!(summary.latest, Some(expected[1].0));
    assert!(summary.disk_bytes > 0);
    let range = (1_000, 2_001).into();
    let HistoryResponse::Days(directory) = client
        .history(HistoryRequest::Days {
            range,
            cursor: None,
        })
        .await
    else {
        panic!("应返回日目录");
    };
    assert!(directory.next.is_none());
    let [DirectoryEntry::Available(day)] = directory.entries.as_slice() else {
        panic!("应只返回一个可用日期：{:?}", directory.entries);
    };
    assert_eq!(day.day, 0);
    assert!(day.data_range.unwrap().intersects(range));
    let read = |after| HistoryRequest::Blocks {
        day: day.day,
        day_id: day.day_id,
        after,
        through: day.published_through,
    };
    assert_error(
        client.history(read(1)).await,
        HistoryErrorCode::InvalidCursor,
    );
    let HistoryResponse::Blocks(page) = client.history(read(0)).await else {
        panic!("应返回已封存的块页");
    };
    assert_eq!(page.offset, 0);
    assert_eq!(page.next_offset, day.published_through);
    assert_eq!(page.through, day.published_through);
    assert!(page.tail_cursor.is_none());
    let bytes = page_bytes(&page);
    let header = DayHeader::decode(&bytes).unwrap();
    assert_eq!((header.utc_day, header.day_id), (day.day, day.day_id));
    assert_eq!(decoded_samples(&bytes, FileKind::Day), expected);
    client.close(server).await;
}

#[tokio::test]
async fn history_requires_extended_login_and_settings_write_requires_admin() {
    let dir = TempDir::new();
    let key = SigningKey::from_bytes(&[28; 32]);
    let (_store, state, pin) = fixture(&dir.0, &key);
    let (mut client, server) = Client::connect(state.clone(), Some(&pin)).await;
    // 协商成功本身不等于登录；复用旧版签名格式也不能获得扩展权限。
    assert_error(
        client.history(HistoryRequest::Summary).await,
        HistoryErrorCode::Unauthorized,
    );
    assert!(matches!(
        client.login(&key, false).await,
        Response::Error(_)
    ));
    assert_error(
        client.history(HistoryRequest::Summary).await,
        HistoryErrorCode::Unauthorized,
    );
    assert!(matches!(client.login(&key, true).await, Response::Ok));
    assert!(matches!(
        client.history(HistoryRequest::Summary).await,
        HistoryResponse::Summary(_)
    ));
    // 已登录但不在历史管理员名单；必须先拒绝权限，不能进入修订检查或配置写入。
    assert_error(
        client
            .history(HistoryRequest::SetSettings {
                expected_revision: SettingsRevision {
                    number: u64::MAX,
                    digest: [0; 32],
                },
                history: EditableHistory {
                    enabled: false,
                    idle_interval_ms: 5_000,
                    min_interval_ms: 250,
                    seal_interval_secs: 300,
                    retention_days: 30,
                    retention_bytes: 0,
                },
            })
            .await,
        HistoryErrorCode::Unauthorized,
    );
    client.close(server).await;

    crate::handlers::register_default_handlers();
    let (mut legacy, server) = Client::connect(state, None).await;
    assert!(matches!(legacy.login(&key, false).await, Response::Ok));
    let services = legacy.request(Request::ListServices).await;
    assert!(
        matches!(&services, Response::Services(items) if items.is_empty()),
        "旧版连接应能查询服务列表，实际返回 {services:?}"
    );
    assert_error(
        legacy.history(HistoryRequest::Summary).await,
        HistoryErrorCode::Unauthorized,
    );
    legacy.close(server).await;
}
