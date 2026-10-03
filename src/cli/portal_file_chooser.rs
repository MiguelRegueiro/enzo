use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

const PORTAL_DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const FILE_CHOOSER_INTERFACE: &str = "org.freedesktop.portal.FileChooser";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";

pub(crate) fn choose_file() -> Result<Option<String>> {
    let connection = Connection::session().context("file chooser unavailable: no session D-Bus")?;
    let token = request_token();
    let request_path = expected_request_path(&connection, &token)?;
    let request = Proxy::new(
        &connection,
        PORTAL_DESTINATION,
        request_path.as_str(),
        REQUEST_INTERFACE,
    )
    .context("file chooser unavailable: could not prepare portal request")?;
    let mut responses = request
        .receive_signal("Response")
        .context("file chooser unavailable: could not listen for portal response")?;

    let portal = Proxy::new(
        &connection,
        PORTAL_DESTINATION,
        PORTAL_PATH,
        FILE_CHOOSER_INTERFACE,
    )
    .context("file chooser unavailable: could not connect to the XDG portal")?;
    let mut options = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("multiple", Value::from(false));
    let actual_path: OwnedObjectPath = portal
        .call("OpenFile", &("", "Choose a video file", options))
        .context("file chooser unavailable: could not open the XDG file chooser")?;
    if actual_path.as_str() != request_path {
        bail!("file chooser unavailable: portal returned an unexpected request handle");
    }

    let message = responses
        .next()
        .context("file chooser unavailable: portal stopped before responding")?;
    let (response, results): (u32, HashMap<String, OwnedValue>) = message
        .body()
        .deserialize()
        .context("file chooser unavailable: invalid portal response")?;
    match response {
        0 => selected_uri(results),
        1 | 2 => Ok(None),
        _ => bail!("file chooser failed with portal response {response}"),
    }
}

fn request_token() -> String {
    format!("enzo_{}", std::process::id())
}

fn expected_request_path(connection: &Connection, token: &str) -> Result<String> {
    let sender = connection
        .unique_name()
        .context("file chooser unavailable: session D-Bus assigned no unique name")?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    Ok(format!(
        "/org/freedesktop/portal/desktop/request/{sender}/{token}"
    ))
}

fn selected_uri(mut results: HashMap<String, OwnedValue>) -> Result<Option<String>> {
    let uris = results
        .remove("uris")
        .context("file chooser returned no selected file")?;
    let uris: Vec<String> = uris
        .try_into()
        .context("file chooser returned invalid selected-file data")?;
    Ok(uris.into_iter().next())
}
