use std::collections::HashMap;

use crate::{
    Catalog, DAY_HEADER_LEN, Error, Id, Marker, MarkerKind, Packet, PacketTag, RawPacket, Result,
    Session,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Wal,
    Day,
}

/// 记录同一物理文件中已读取的定义。请从头按文件顺序读取；
/// 随机读取时，先按顺序加载所需的前置定义。
/// 调用方仍需校验实际包边界、文件大小及持久化状态。
#[derive(Debug)]
pub struct FileContext {
    kind: FileKind,
    last_offset: Option<u64>,
    last_catalog: Option<u32>,
    last_session: Option<u64>,
    catalogs: HashMap<u32, Catalog>,
    sessions: HashMap<u64, Session>,
    boot_ids: HashMap<Id, Option<Id>>,
}

impl FileContext {
    pub fn new(kind: FileKind) -> Self {
        Self {
            kind,
            last_offset: None,
            last_catalog: None,
            last_session: None,
            catalogs: HashMap::new(),
            sessions: HashMap::new(),
            boot_ids: HashMap::new(),
        }
    }

    pub fn session(&self, offset: u64) -> Option<&Session> {
        self.sessions.get(&offset)
    }
    pub fn catalog(&self, version: u32) -> Option<&Catalog> {
        self.catalogs.get(&version)
    }
    pub fn current_session(&self) -> Option<(u64, &Session)> {
        let offset = self.last_session?;
        Some((offset, self.session(offset)?))
    }
    pub fn current_catalog(&self) -> Option<&Catalog> {
        self.catalog(self.last_catalog?)
    }

    pub fn decode_at(&mut self, offset: u64, raw: &RawPacket<'_>) -> Result<Packet> {
        if offset < DAY_HEADER_LEN as u64 || self.last_offset.is_some_and(|last| offset <= last) {
            return Err(Error::Invalid("unordered file packet offsets"));
        }
        let catalog = match raw.tag {
            PacketTag::Frame if self.kind == FileKind::Wal => {
                self.current_session().ok_or(Error::Session)?;
                self.current_catalog()
            }
            PacketTag::Block if self.kind == FileKind::Day => {
                let header = raw.block_header()?.expect("BLOCK tag checked");
                self.session(header.session_ref).ok_or(Error::Session)?;
                self.catalog(header.catalog_version)
            }
            PacketTag::Block | PacketTag::Frame => {
                return Err(Error::Invalid("packet not allowed in this file kind"));
            }
            _ => None,
        };
        let packet = raw.decode(catalog)?;
        match &packet {
            Packet::Catalog(catalog) => {
                if let Some(existing) = self.catalog(catalog.version) {
                    if existing != catalog {
                        return Err(Error::Catalog);
                    }
                } else {
                    self.catalogs.insert(catalog.version, catalog.clone());
                }
                self.last_catalog = Some(catalog.version);
            }
            Packet::Marker(Marker::Session(session)) => {
                if self
                    .boot_ids
                    .get(&session.session_id)
                    .is_some_and(|boot_id| *boot_id != session.boot_id)
                {
                    return Err(Error::Session);
                }
                self.boot_ids.insert(session.session_id, session.boot_id);
                self.sessions.insert(offset, *session);
                self.last_session = Some(offset);
            }
            Packet::Marker(Marker::Event(event)) => {
                if self.kind == FileKind::Day && event.kind == MarkerKind::Continuation {
                    return Err(Error::Invalid("WAL continuation in day file"));
                }
                self.session(event.session_ref).ok_or(Error::Session)?;
            }
            _ => {}
        }
        self.last_offset = Some(offset);
        Ok(packet)
    }
}
