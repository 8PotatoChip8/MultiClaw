# Runbook

## Host Level Checks
### Are the Host services running?
```bash
sudo systemctl status multiclaw-stack
docker compose -f /opt/multiclaw/docker-compose.yml ps
sudo systemctl status ollama
```

### Rotating Master Key
See internal CLI: `multiclaw init rotate-keys`

## VM Level Checks
```bash
incus list multiclaw
incus shell mc-mainhc-ceo-1
# from inside the VM
systemctl status openclaw-gateway
systemctl status multiclaw-agentd
```
