use anyhow::Context;

pub fn get_client_version() -> anyhow::Result<[u8; 3]> {
    let major: u8 = env!("CARGO_PKG_VERSION_MAJOR")
        .parse()
        .context("malformed major version")?;

    let minor: u8 = env!("CARGO_PKG_VERSION_MINOR")
        .parse()
        .context("malformed minor version")?;

    let patch: u8 = env!("CARGO_PKG_VERSION_PATCH")
        .parse()
        .context("malformed patch version")?;

    Ok([major, minor, patch])
}

