use std::{error::Error, sync::Arc};

use ssh_ui::{
    cursive::views::{Dialog, TextView},
    key::KeyPair,
    App, AppServer, AppSession,
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
        _pub_key: ssh_ui::key::PublicKey,
    ) -> Result<Box<dyn cursive::View>, Box<dyn Error>> {
        println!("on_start");
        Ok(Box::new(
            Dialog::around(TextView::new("Hello over ssh!"))
                .title("ssh_ui")
                .button("Quit", |s| s.quit()),
        ))
    }

    fn on_end(&mut self) -> Result<(), Box<dyn Error>> {
        println!("on_end");
        Ok(())
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
    let key_pair = KeyPair::generate_ed25519().unwrap();
    let mut server = AppServer::new_with_port(2222);
    let app = DialogApp {};
    server.run(key_pair, Arc::new(app)).await.unwrap();
}
