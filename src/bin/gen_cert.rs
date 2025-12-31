use rcgen::generate_simple_self_signed;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subject_alt_names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "frp-egg.com".to_string(),
    ];
    let cert = generate_simple_self_signed(subject_alt_names)?;

    fs::write("cert.pem", cert.cert.pem())?;
    fs::write("key.pem", cert.signing_key.serialize_pem())?;

    println!("Generated cert.pem and key.pem");
    Ok(())
}
