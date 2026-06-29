use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LinkVersion {
    pub api_version: &'static str,
    pub protocol_version: u32,
    pub min_protocol_version: u32,
}

pub const LINK_VERSION: LinkVersion = LinkVersion {
    api_version: "v1",
    protocol_version: 1,
    min_protocol_version: 1,
};
