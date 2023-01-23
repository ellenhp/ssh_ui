use russh::server::Auth;
use russh::server::Handler;
use russh::server::Session;
use russh::ChannelId;
use russh_keys::key::PublicKey;
use std::future::Future;
use std::pin::Pin;
use tokio::spawn;
use tokio::sync::mpsc::Sender;

use super::session_manager::SessionRepoUpdate;
use super::session_manager::SshSessionUpdate;

pub struct ThinHandler {
    session_repo_update_sender: Sender<SessionRepoUpdate>,
    session_update_sender: Option<Sender<SshSessionUpdate>>,
    pubkey: Option<PublicKey>,
}

impl ThinHandler {
    pub(crate) fn new(session_repo_update_sender: Sender<SessionRepoUpdate>) -> ThinHandler {
        ThinHandler {
            session_repo_update_sender,
            session_update_sender: None,
            pubkey: None,
        }
    }
}

impl Handler for ThinHandler {
    type FutureAuth = Pin<Box<dyn Future<Output = Result<(Self, Auth), Self::Error>> + Send>>;
    type FutureUnit = Pin<Box<dyn Future<Output = Result<(Self, Session), Self::Error>> + Send>>;
    type FutureBool =
        Pin<Box<dyn Future<Output = Result<(Self, Session, bool), Self::Error>> + Send>>;

    fn channel_open_session(mut self, channel: ChannelId, session: Session) -> Self::FutureBool {
        let (session_update_sender, session_update_receiver) = tokio::sync::mpsc::channel(100);
        self.session_update_sender = Some(session_update_sender);
        let sender = self.session_repo_update_sender.clone();
        let handle = session.handle();
        let pubkey = self.pubkey.clone().expect("pubkey not set");
        spawn(async move {
            sender
                .send(SessionRepoUpdate::NewSession(
                    handle,
                    channel,
                    session_update_receiver,
                    pubkey,
                ))
                .await
                .unwrap();
        });

        self.finished_bool(true, session)
    }

    fn finished_auth(self, auth: Auth) -> Self::FutureAuth {
        Box::pin(async move { Ok((self, auth)) })
    }

    fn finished_bool(self, b: bool, session: Session) -> Self::FutureBool {
        Box::pin(async move { Ok((self, session, b)) })
    }

    fn finished(self, session: Session) -> Self::FutureUnit {
        Box::pin(async move { Ok((self, session)) })
    }

    fn auth_none(self, _user: &str) -> Self::FutureAuth {
        self.finished_auth(Auth::Reject {
            proceed_with_methods: None,
        })
    }

    fn channel_close(self, _channel: ChannelId, session: Session) -> Self::FutureUnit {
        let sender = self.session_update_sender.clone().unwrap();
        spawn(async move {
            sender.send(SshSessionUpdate::Close).await.unwrap();
        });
        self.finished(session)
    }

    fn auth_publickey(mut self, _user: &str, public_key: &PublicKey) -> Self::FutureAuth {
        self.pubkey = Some(public_key.clone());
        self.finished_auth(Auth::Accept)
    }

    fn data(self, _channel: ChannelId, data: &[u8], session: Session) -> Self::FutureUnit {
        let data = data.to_vec();
        let sender = self.session_update_sender.clone();
        spawn(async move {
            sender
                .clone()
                .unwrap()
                .send(SshSessionUpdate::Data(data.to_vec()))
                .await
                .unwrap();
        });
        self.finished(session)
    }

    #[allow(unused_variables)]
    fn shell_request(self, channel: ChannelId, session: Session) -> Self::FutureUnit {
        self.finished(session)
    }

    #[allow(unused_variables)]
    fn pty_request(
        self,
        channel: ChannelId,
        term: &str,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        modes: &[(russh::Pty, u32)],
        session: Session,
    ) -> Self::FutureUnit {
        let sender = self.session_update_sender.clone();
        spawn(async move {
            sender
                .clone()
                .unwrap()
                .send(SshSessionUpdate::WindowResize(
                    col_width as usize,
                    row_height as usize,
                ))
                .await
                .unwrap();
        });
        self.finished(session)
    }

    #[allow(unused_variables)]
    fn window_change_request(
        self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        session: Session,
    ) -> Self::FutureUnit {
        let sender = self.session_update_sender.clone();
        spawn(async move {
            sender
                .clone()
                .unwrap()
                .send(SshSessionUpdate::WindowResize(
                    col_width as usize,
                    row_height as usize,
                ))
                .await
                .unwrap();
        });
        self.finished(session)
    }

    type Error = anyhow::Error;
}

impl Drop for ThinHandler {
    fn drop(&mut self) {}
}
