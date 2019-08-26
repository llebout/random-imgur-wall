# random-imgur-wall

This is source code for the website at https://imgur.schmilblick.org

## How to run your own locally

Install Rust with Cargo for your system. You can use https://rustup.rs/ or install your system packages.

Run these commands in a unix shell:
```
cargo install --force cargo-web
git clone https://github.com/leo-lb/random-imgur-wall
cd random-imgur-wall/web
cargo web start --release --port 8001 --host 127.0.0.1 &
cd ../server
WS_LISTEN_ADDR="127.0.0.1:8002" cargo run --release &
```

Now you can access it at http://localhost:8001

To stop it, run the following in the same unix shell as earlier:
```
kill $(jobs -p)
```

---

If you want to deploy this on a server with your own domain, remember to edit the config.json file to point to your websocket server. (https://github.com/leo-lb/random-imgur-wall/blob/master/web/static/config.json)
