# Inert website

Static HTML exported from the Claude Design canvas
(https://claude.ai/artifact/3CT8d6uFZxtVjHDn2DgFbq): `index.html` (Browser, blue)
and `protocol.html` (Protocol, dark), screenshots in `shots/`, the icon from
`inert-browser --write-icon`.

Hosted on the homeserver (citadel) in `~/inert-site`: Caddy serves `public/`,
and a Cloudflare quick tunnel publishes it (no domain yet; the address changes
when the tunnel restarts). Deploy:

    rsync -a --delete index.html protocol.html icon.png shots eren@192.168.0.14:~/inert-site/public/
    rsync -a Caddyfile docker-compose.yml eren@192.168.0.14:~/inert-site/
    ssh eren@192.168.0.14 'cd ~/inert-site && docker compose up -d'
    ssh eren@192.168.0.14 'cd ~/inert-site && docker compose logs cloudflared | grep trycloudflare.com'

With a domain: create a named tunnel in Cloudflare, put its config in
`cloudflared/`, and switch the compose command to
`tunnel --no-autoupdate --config /etc/cloudflared/config.yml run` (as gut-site does).
