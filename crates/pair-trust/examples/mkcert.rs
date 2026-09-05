fn main() -> anyhow::Result<()> {
    let id = pair_trust::Identity::generate_with_uuid("8daa6983-8cdc-4e38-afe5-c076daf205ae")?;
    print!("{}", id.cert_pem);
    Ok(())
}
