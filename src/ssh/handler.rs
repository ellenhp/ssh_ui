use std::future::Future;
use std::pin::Pin;
use thrussh::server::Auth;
use thrussh::server::Handler;
use thrussh::server::Session;
use thrussh::ChannelId;
use thrussh_keys::key::ed25519;
use thrussh_keys::key::OpenSSLPKey;
use thrussh_keys::key::PublicKey;
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

    fn channel_open_session(mut self, channel: ChannelId, session: Session) -> Self::FutureUnit {
        let (session_update_sender, session_update_receiver) = tokio::sync::mpsc::channel(100);
        self.session_update_sender = Some(session_update_sender);
        let sender = self.session_repo_update_sender.clone();
        let handle = session.handle();
        let key = clone_option_public_key(&self.pubkey);
        spawn(async move {
            sender
                .send(SessionRepoUpdate::NewSession(
                    handle,
                    channel,
                    session_update_receiver,
                    key,
                ))
                .await
                .unwrap();
        });

        self.finished(session)
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
        self.finished_auth(Auth::Reject)
    }

    fn channel_close(self, _channel: ChannelId, session: Session) -> Self::FutureUnit {
        let sender = self.session_update_sender.clone().unwrap();
        spawn(async move {
            sender.send(SshSessionUpdate::Close).await.unwrap();
        });
        self.finished(session)
    }

    fn auth_publickey(mut self, _user: &str, public_key: &PublicKey) -> Self::FutureAuth {
        self.pubkey = Some(clone_public_key(public_key));
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
        modes: &[(thrussh::Pty, u32)],
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

fn clone_public_key(key: &PublicKey) -> PublicKey {
    match key {
        PublicKey::Ed25519(a) => PublicKey::Ed25519(ed25519::PublicKey { key: a.key.clone() }),
        PublicKey::RSA { key, hash } => PublicKey::RSA {
            key: OpenSSLPKey(key.0.clone()),
            hash: hash.clone(),
        },
    }
}

fn clone_option_public_key(key: &Option<PublicKey>) -> PublicKey {
    match key {
        Some(PublicKey::Ed25519(a)) => {
            PublicKey::Ed25519(ed25519::PublicKey { key: a.key.clone() })
        }
        Some(PublicKey::RSA { key, hash }) => PublicKey::RSA {
            key: OpenSSLPKey(key.0.clone()),
            hash: hash.clone(),
        },
        None => panic!("No public key"),
    }
}
