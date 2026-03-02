use anyhow::Ok;
use log::info;
use log::trace;
use russh::keys::PublicKey;
use russh::server::Auth;
use russh::server::Handler;
use russh::server::Msg;
use russh::server::Session;
use russh::Channel;
use russh::ChannelId;
use russh::MethodSet;
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
    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
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

        Ok(true)
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        info!(
            "Public key auth request for user {} using key {:?}",
            user, public_key
        );
        if user == "root" {
            Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        } else {
            self.pubkey = Some(public_key.clone());
            Ok(Auth::Accept)
        }
    }

    async fn auth_none(&mut self, user: &str) -> Result<Auth, Self::Error> {
        info!("`None` auth request for user {} using", user);
        let mut allowed_methods = MethodSet::empty();
        allowed_methods.push(russh::MethodKind::PublicKey);
        match user {
            // This might be fun for a honeypot but anyone looking for `none` auth with a root user deserves to be shut down.
            "root" => Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            }),
            "anon" | "anonymous" => Ok(Auth::Accept),
            _ => Ok(Auth::Reject {
                proceed_with_methods: Some(allowed_methods),
                partial_success: false,
            }),
        }
    }

    async fn channel_close(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let sender = self.session_update_sender.clone().unwrap();
        spawn(async move {
            sender.send(SshSessionUpdate::Close).await.unwrap();
        });
        Result::Ok(())
    }

    async fn data(
        &mut self,
        _channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
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
        Result::Ok(())
    }

    async fn shell_request(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        info!("shell request");
        Result::Ok(())
    }

    async fn pty_request(
        &mut self,
        _channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
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
        Result::Ok(())
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
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
        Result::Ok(())
    }

    type Error = anyhow::Error;
}

impl Drop for ThinHandler {
    fn drop(&mut self) {}
}
