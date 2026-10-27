#!/usr/bin/env bash
# staging-letsencrypt.sh — give the staging box a publicly trusted server cert.
#
#   ./scripts/staging-letsencrypt.sh            # install certbot, issue, wire renewals
#
# Idempotent; rerun after the box is rebuilt. Over SSM, as root (Amazon Linux
# 2023, no certbot in its repos): certbot + certbot-dns-route53 in a venv under
# /opt/certbot, `certonly --dns-route53` for the staging hostname (RSA key so
# the chain is R1x -> ISRG Root X1, the root the boards pin via
# certs/ca/staging-bundle.pem), a deploy hook that copies fullchain/privkey to
# /etc/supervictor/{server.pem,server.key} (the unit loads those as
# credentials, no unit change) and restarts the service, and a systemd timer
# running `certbot renew` twice a day. The fleet-signed server cert is kept at
# /etc/supervictor/server.{pem,key}.fleet.
#
# Needs the AcmeDns01Route53 policy on the instance role (template-staging.yaml;
# staging-stack-update.sh applies template changes). The mTLS CLIENT CA is
# untouched: devices and admins still chain to ca.pem. No ACME contact email is
# registered (Let's Encrypt no longer emails expiry notices).
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=letsencrypt
need aws curl openssl
need_admin
cd "$REPO_ROOT"
INSTANCE_ID="$(staging_instance)"

# read -d '' instead of $(cat <<EOF): macOS bash 3.2 cannot parse quotes
# inside nested heredocs within a command substitution.
read -r -d '' REMOTE <<'REMOTE_EOF' || true
set -euo pipefail
echo "== certbot venv"
dnf install -y -q python3-pip augeas-libs >/dev/null 2>&1 || true
[ -x /opt/certbot/bin/certbot ] || {
    python3 -m venv /opt/certbot
    /opt/certbot/bin/pip install -q --upgrade pip
    /opt/certbot/bin/pip install -q certbot certbot-dns-route53
}
ln -sf /opt/certbot/bin/certbot /usr/local/bin/certbot
certbot --version
echo "== deploy hook"
install -d -m 0755 /etc/letsencrypt/renewal-hooks/deploy
cat > /etc/letsencrypt/renewal-hooks/deploy/supervictor.sh <<'HOOK'
#!/usr/bin/env bash
# certbot deploy hook: new cert -> the files the unit loads as credentials.
set -euo pipefail
LIVE=/etc/letsencrypt/live/__HOST__
[ -f /etc/supervictor/server.pem.fleet ] || cp -p /etc/supervictor/server.pem /etc/supervictor/server.pem.fleet
[ -f /etc/supervictor/server.key.fleet ] || cp -p /etc/supervictor/server.key /etc/supervictor/server.key.fleet
install -m 0644 "$LIVE/fullchain.pem" /etc/supervictor/server.pem
install -m 0600 "$LIVE/privkey.pem"   /etc/supervictor/server.key
systemctl restart supervictor-endpoint
echo "[hook] supervictor-endpoint restarted with $(openssl x509 -in /etc/supervictor/server.pem -noout -enddate)"
HOOK
sed -i "s/__HOST__/__HOSTNAME_FQDN__/" /etc/letsencrypt/renewal-hooks/deploy/supervictor.sh
chmod 0755 /etc/letsencrypt/renewal-hooks/deploy/supervictor.sh
echo "== issue"
certbot certonly --non-interactive --agree-tos --register-unsafely-without-email \
    --dns-route53 --key-type rsa --rsa-key-size 2048 \
    -d __HOSTNAME_FQDN__ --deploy-hook /etc/letsencrypt/renewal-hooks/deploy/supervictor.sh
echo "== renew timer"
cat > /etc/systemd/system/certbot-renew.service <<'UNIT'
[Unit]
Description=Renew Let's Encrypt certificates (certbot)
[Service]
Type=oneshot
ExecStart=/opt/certbot/bin/certbot renew --quiet
UNIT
cat > /etc/systemd/system/certbot-renew.timer <<'UNIT'
[Unit]
Description=Run certbot renew twice daily
[Timer]
OnCalendar=*-*-* 03,15:17:00
RandomizedDelaySec=1h
Persistent=true
[Install]
WantedBy=timers.target
UNIT
systemctl daemon-reload
systemctl enable --now certbot-renew.timer
systemctl list-timers certbot-renew.timer --no-pager | head -2
echo "== result"
systemctl is-active supervictor-endpoint
openssl x509 -in /etc/supervictor/server.pem -noout -subject -issuer -enddate
journalctl -u supervictor-endpoint -n 3 --no-pager
REMOTE_EOF
REMOTE="${REMOTE//__HOSTNAME_FQDN__/$STAGING_HOST}"

log "issuing for $STAGING_HOST on $INSTANCE_ID (pip install takes a minute or two)"
ssm_run "$INSTANCE_ID" "letsencrypt for $STAGING_HOST" 900 <<<"$REMOTE" | tail -30

log "verifying with the system trust store (no --cacert):"
admin_curl -m 10 -o /dev/null -w '  /fleet/summary %{http_code}\n' "https://$STAGING_HOST/fleet/summary"
echo | openssl s_client -connect "$STAGING_HOST:443" -servername "$STAGING_HOST" 2>/dev/null | openssl x509 -noout -issuer -enddate
