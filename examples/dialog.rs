use std::{error::Error, sync::Arc};

use cursive::Cursive;
use russh_keys::key::SignatureHash;
use ssh_ui::{
    cursive::views::{Dialog, TextView},
    russh_keys::key::{KeyPair, PublicKey},
    App, AppServer, AppSession, SessionHandle,
};

struct DialogAppSession {}

impl DialogAppSession {
    pub fn new() -> Self {
        Self {}
    }
}

impl AppSession for DialogAppSession {
    fn on_start(
        &mut self,
        _siv: &mut Cursive,
        _session_handle: SessionHandle,
        _pub_key: PublicKey,
    ) -> Result<Box<dyn cursive::View>, Box<dyn Error>> {
        println!("on_start");
        Ok(Box::new(
            Dialog::around(TextView::new("Hello over ssh!"))
                .title("ssh_ui")
                .button("Quit", |s| s.quit()),
        ))
    }
}

struct DialogApp {}

impl App for DialogApp {
    fn on_load(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn new_session(&self) -> Box<dyn AppSession> {
        Box::new(DialogAppSession::new())
    }
}

#[tokio::main]
async fn main() {
    let key_pair = KeyPair::generate_rsa(3072, SignatureHash::SHA2_256).unwrap();
    let mut server = AppServer::new_with_port(2222);
    let app = DialogApp {};
    server.run(&[key_pair], Arc::new(app)).await.unwrap();
}
