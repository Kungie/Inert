# Inert website

Plain static HTML: `index.html` (Browser, blue) and `protocol.html` (Protocol,
dark), screenshots in `shots/`, the icon from `inert-browser --write-icon`.
No build step and no scripts beyond the page itself. Live at
https://inert.erenkahraman.dev.

## Self-hosting

`docker-compose.yml` runs Caddy for the files and a named Cloudflare Tunnel in
front of it, so no port has to be opened. You need a domain on Cloudflare and
`cloudflared` logged in once (`cloudflared tunnel login`). `HOST` is your
server, `HOSTNAME` the site's address.

1. Create the tunnel and point the hostname at it:

       cloudflared tunnel create inert
       cloudflared tunnel route dns inert HOSTNAME

2. Copy the files to the server:

       rsync -a --delete index.html protocol.html icon.png shots HOST:~/inert-site/public/
       rsync -a Caddyfile docker-compose.yml HOST:~/inert-site/

3. Put the tunnel's credentials (`~/.cloudflared/<id>.json`) on the server as
   `~/inert-site/cloudflared/tunnel.json` (mode 400, owned by the uid in
   `docker-compose.yml`), and `cloudflared/config.example.yml` as
   `~/inert-site/cloudflared/config.yml` with the tunnel id and hostname.

4. Start it:

       ssh HOST 'cd ~/inert-site && docker compose up -d'

To update the site later, repeat the first `rsync`; Caddy serves the new files
at once.
