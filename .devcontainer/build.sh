#!/usr/bin/env bash
# Build (and optionally push) the Three Rings dev image, dgoings/three-rings.
#
# Usage:
#   .devcontainer/build.sh              # build + tag dgoings/three-rings:latest (this host's arch)
#   .devcontainer/build.sh --push       # ...and push to Docker Hub (needs `docker login`)
#   .devcontainer/build.sh --multiarch  # build linux/amd64 + linux/arm64 and push (implies --push)
#
# Env overrides:
#   IMAGE=dgoings/three-rings:v2 .devcontainer/build.sh
#   PLATFORMS=linux/amd64 .devcontainer/build.sh --multiarch
#
# Why --multiarch always pushes: a multi-platform build produces a manifest list,
# and the local Docker image store cannot hold one — buildx can only export it to
# a registry. Single-arch builds still land in the local daemon as before, which
# is what `devcontainer.json` (image: dgoings/three-rings:latest) picks up.
set -euo pipefail

IMAGE="${IMAGE:-dgoings/three-rings:latest}"
PLATFORMS="${PLATFORMS:-linux/amd64,linux/arm64}"
BUILDER="${BUILDER:-three-rings-builder}"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

MULTIARCH=false
PUSH=false
for arg in "$@"; do
  case "${arg}" in
    --multiarch) MULTIARCH=true; PUSH=true ;;
    --push)      PUSH=true ;;
    -h|--help)   awk 'NR>1 && /^#/ {sub(/^# ?/, ""); print; next} NR>1 {exit}' "${BASH_SOURCE[0]}"; exit 0 ;;
    *)           echo "Unknown argument: ${arg}" >&2; exit 2 ;;
  esac
done

if [[ "${MULTIARCH}" == true ]]; then
  # The default buildx builder uses the `docker` driver, which cannot do
  # multi-platform builds. Create a `docker-container` builder once and reuse it.
  if ! docker buildx inspect "${BUILDER}" >/dev/null 2>&1; then
    echo "Creating buildx builder ${BUILDER} (docker-container driver) ..."
    docker buildx create --name "${BUILDER}" --driver docker-container --bootstrap >/dev/null
  fi

  echo "Building ${IMAGE} for ${PLATFORMS} from ${DIR}/Dockerfile (pushing to registry) ..."
  docker buildx build \
    --builder "${BUILDER}" \
    --platform "${PLATFORMS}" \
    --push \
    -t "${IMAGE}" \
    -f "${DIR}/Dockerfile" \
    "${DIR}"
else
  echo "Building ${IMAGE} from ${DIR}/Dockerfile ..."
  docker build -t "${IMAGE}" -f "${DIR}/Dockerfile" "${DIR}"

  if [[ "${PUSH}" == true ]]; then
    echo "Pushing ${IMAGE} ..."
    docker push "${IMAGE}"
  fi
fi

echo "Done: ${IMAGE}"
