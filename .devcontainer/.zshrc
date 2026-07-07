# Three Rings devcontainer shell config (bind-mounted to /home/vscode/.zshrc).
export PATH="$HOME/.cargo/bin:$PATH"
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

export CARGO_TERM_COLOR=always

autoload -Uz compinit && compinit -u
setopt PROMPT_SUBST
PROMPT='%F{cyan}three-rings%f:%F{yellow}%~%f %# '
