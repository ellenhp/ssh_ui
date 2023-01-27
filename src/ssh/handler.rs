use anyhow::Ok;
use log::info;
use log::trace;
use russh::server::Auth;
use russh::server::Handler;
use russh::server::Msg;
use russh::server::Session;
use russh::Channel;
use russh::ChannelId;
use russh::MethodSet;
use russh_keys::key::PublicKey;
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

#[async_trait::async_trait]
impl Handler for ThinHandler {
    async fn channel_open_session(
        mut self,
        channel: Channel<Msg>,
        session: Session,
    ) -> Result<(Self, bool, Session), Self::Error> {
        info!("Channel opened");
        let (session_update_sender, session_update_receiver) = tokio::sync::mpsc::channel(100);
        self.session_update_sender = Some(session_update_sender);
        let sender = self.session_repo_update_sender.clone();
        let handle = session.handle();
        let pubkey = self.pubkey.clone();
        spawn(async move {
            sender
                .send(SessionRepoUpdate::NewSession(
                    handle,
                    channel.id(),
                    session_update_receiver,
                    pubkey,
                ))
                .await
                .unwrap();
        });

        Ok((self, true, session))
    }

    async fn auth_publickey(
        mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<(Self, Auth), Self::Error> {
        info!(
            "Public key auth request for user {} using key {:?}",
            user, public_key
        );
        if user == "root" {
            Ok((
                self,
                Auth::Reject {
                    proceed_with_methods: None,
                },
            ))
        } else {
            self.pubkey = Some(public_key.clone());
            Ok((self, Auth::Accept))
        }
    }

    async fn auth_none(self, user: &str) -> Result<(Self, Auth), Self::Error> {
        info!("`None` auth request for user {} using", user);
        match user {
            // This might be fun for a honeypot but anyone looking for `none` auth with a root user deserves to be shut down.
            "root" => Ok((
                self,
                Auth::Reject {
                    proceed_with_methods: None,
                },
            )),
            "anon" | "anonymous" => Ok((self, Auth::Accept)),
            _ => Ok((
                self,
                Auth::Reject {
                    proceed_with_methods: Some(MethodSet::PUBLICKEY),
                },
            )),
        }
    }

    async fn channel_close(
        self,
        _channel: ChannelId,
        session: Session,
    ) -> Result<(Self, Session), Self::Error> {
        let sender = self.session_update_sender.clone().unwrap();
        spawn(async move {
            sender.send(SshSessionUpdate::Close).await.unwrap();
        });
        Result::Ok((self, session))
    }

    async fn data(
        self,
        _channel: ChannelId,
        data: &[u8],
        session: Session,
    ) -> Result<(Self, Session), Self::Error> {
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
        Result::Ok((self, session))
    }

    async fn shell_request(
        self,
        _channel: ChannelId,
        session: Session,
    ) -> Result<(Self, Session), Self::Error> {
        info!("shell request");
        Result::Ok((self, session))
    }

    async fn pty_request(
        self,
        _channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: Session,
    ) -> Result<(Self, Session), Self::Error> {
        info!("pty request");
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
        Result::Ok((self, session))
    }

    async fn window_change_request(
        self,
        _channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        session: Session,
    ) -> Result<(Self, Session), Self::Error> {
        trace!("window change request");
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
        Result::Ok((self, session))
    }

    type Error = anyhow::Error;
}

impl Drop for ThinHandler {
    fn drop(&mut self) {}
}
