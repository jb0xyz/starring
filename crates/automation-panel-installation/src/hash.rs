use std::fmt::Write;

use automation_state::{ButtonRoute, PanelSpec};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
struct PanelPresentation<'a> {
    render_revision: u32,
    content: &'a str,
    buttons: Vec<ButtonPresentation<'a>>,
}

#[derive(Serialize)]
struct ButtonPresentation<'a> {
    label: &'a str,
    route: String,
}

pub fn spec_hash(render_revision: u32, spec: &PanelSpec) -> String {
    let projection = PanelPresentation {
        render_revision,
        content: &spec.content,
        buttons: spec
            .buttons
            .iter()
            .map(|button| ButtonPresentation {
                label: &button.label,
                route: route_token(&button.route),
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&projection).expect("panel presentation serializes");
    hex_sha256(&bytes)
}

fn route_token(route: &ButtonRoute) -> String {
    match route {
        ButtonRoute::Static { key } => format!("static:{key}"),
        ButtonRoute::InstanceAction { action, .. } => format!("instance_action:{action}"),
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a string succeeds");
    }
    output
}
