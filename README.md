# ssh_ui

`ssh_ui` helps you painlessly turn a [cursive](https://crates.io/crates/cursive)-based terminal UI (TUI) into an application accessible over ssh. Designed to make the creation of BBS systems or ssh-based games simple, `ssh_ui` takes a minimally opinionated approach to opening a TUI up to remote connections, beyond requiring you to use `cursive`. The ssh server implementation is provided by [thrussh](https://crates.io/crates/thrussh).

The `main` function of the simplest `ssh_ui`-based application looks something like this:

```
#[tokio::main]
async fn main() {
    let key_pair = KeyPair::generate_ed25519().unwrap();
    let mut server = AppServer::new_with_port(2222);
    let app = DialogApp {};
    server.run(key_pair, Arc::new(app)).await.unwrap();
}
```

First this generates a new keypair (but you should load one from disk for user-facing installations). Then it initializes a new `AppServer` on port 2222 and  new instance of a `DialogApp`, then calls `AppServer::run` to listen on the specified port for incoming connections. Let's look next at what makes `AppServer` tick.

```
struct DialogApp {}

impl App for DialogApp {
    fn on_load(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn new_session(&self) -> Box<dyn AppSession> {
        Box::new(DialogAppSession::new())
    }
}
```

All it's doing here is providing a new `DialogAppSession` whenever there's a new incoming ssh connection. `DialogAppSession` is implemented as follows:

```
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
        Ok(Box::new(
            Dialog::around(TextView::new("Hello over ssh!"))
                .title("ssh_ui")
                .button("Quit", |s| s.quit()),
        ))
    }

    fn on_end(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}
```

This is where the actual `cursive` TUI is created and returned to `ssh_ui`. You can return whatever TUI you want, and `ssh_ui` will take care of serving it to the client.

## Contributions

If you'd like to use `ssh_ui` and it doesn't quite fit your needs, feel free to open an issue or pull request on the [GitHub repository](https://github.com/ellenhp/ssh_ui).