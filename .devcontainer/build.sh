#!/usr/bin/env bash
# Build (and optionally push) the Three Rings dev image, dgoings/three-rings.
#
# Usage:
#   .devcontainer/build.sh            # build + tag dgoings/three-rings:latest
#   .devcontainer/build.sh --push     # ...and push to Docker Hub (needs `docker login`)
set -euo pipefail

IMAGE="${IMAGE:-dgoings/three-rings:latest}"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Building ${IMAGE} from ${DIR}/Dockerfile ..."
docker build -t "${IMAGE}" -f "${DIR}/Dockerfile" "${DIR}"

if [[ "${1:-}" == "--push" ]]; then
  echo "Pushing ${IMAGE} ..."
  docker push "${IMAGE}"
fi

echo "Done: ${IMAGE}"
