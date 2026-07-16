#!/usr/bin/env bash
#
# Repair GitHub auth inside the devcontainer. Run on every container start via
# devcontainer.json's `postStartCommand`.
#
# Why this exists: VS Code copies the *host's* ~/.gitconfig into the container at
# creation. If the host configured `gh` as its git credential helper, that config
# carries a host-only path — e.g. macOS Homebrew's `!/opt/homebrew/bin/gh auth
# git-credential` — which does not exist in this Debian container. That
# github.com-scoped helper *overrides* the working generic (VS Code-forwarded)
# helper, so `git push` to github.com fails with "could not read Username".
#
# This script strips those host-path helpers and, when a GH_TOKEN is present
# (see .devcontainer/.env), points git at the container's own `gh`. With no token
# it just removes the broken helper and leaves the generic VS Code credential
# forwarding in place. Best-effort: it never fails container start.

set -u

# 1. Drop any github.com / gist credential helper copied from the host. These are
#    the entries that may point at a nonexistent host path.
for host in "https://github.com" "https://gist.github.com"; do
    git config --global --unset-all "credential.${host}.helper" 2>/dev/null || true
done

# 2. If a token is available and gh is installed, make gh the credential helper
#    for github.com (it reads GH_TOKEN at credential time). Otherwise leave the
#    generic helper untouched so VS Code's host-forwarded auth still works.
if [ -n "${GH_TOKEN:-}" ] && command -v gh >/dev/null 2>&1; then
    gh auth setup-git --hostname github.com 2>/dev/null \
        && echo "setup-git-auth: git configured to use gh (GH_TOKEN) for github.com" \
        || echo "setup-git-auth: gh auth setup-git failed (continuing)"
else
    echo "setup-git-auth: no GH_TOKEN or gh; cleared host-path helpers, kept generic credential helper"
fi

exit 0
