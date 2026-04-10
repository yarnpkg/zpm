#!/usr/bin/env bash
set -exou pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
YARN_BIN="${REPO_ROOT}/target/release/yarn-bin"

if [[ ! -x "${YARN_BIN}" ]]; then
  echo "Missing compiled Yarn binary at ${YARN_BIN}" >&2
  exit 1
fi

WORKDIR="$(mktemp -d)"
SERVER_PID=""
cleanup() {
  if [[ -n "${SERVER_PID}" ]]; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  rm -rf "${WORKDIR}"
}
trap cleanup EXIT

mkdir -p "${WORKDIR}/bin"
ln -sf "${YARN_BIN}" "${WORKDIR}/bin/yarn"
export PATH="${WORKDIR}/bin:${PATH}"

PROJECT_DIR="${WORKDIR}/project"
mkdir -p "${PROJECT_DIR}/packages/django-workspace"
cd "${PROJECT_DIR}"

cat > package.json <<'JSON'
{
  "name": "e2e-django-root",
  "private": true,
  "packageManager": "yarn@6.0.0-rc.10",
  "workspaces": [
    "packages/*"
  ]
}
JSON

cat > .yarnrc.yml <<'YAML'
unstableIslands:
  python:
    workspaces:
      - "django-workspace"
    linker: "venv"
YAML

cat > packages/django-workspace/package.json <<'JSON'
{
  "name": "django-workspace",
  "private": true,
  "version": "0.0.0",
  "dependencies": {
    "django": "pypi:>=5.0,<6.0"
  },
  "scripts": {
    "start": "yarn python server.py"
  }
}
JSON

cat > packages/django-workspace/server.py <<'PY'
#!/usr/bin/env python
import sys

from django.conf import settings
from django.core.management import execute_from_command_line
from django.http import HttpResponse
from django.urls import path


def hello_world(_request):
    return HttpResponse("Hello world from Django via pypi protocol!\n", content_type="text/plain")


if not settings.configured:
    settings.configure(
        DEBUG=True,
        SECRET_KEY="e2e-django-demo",
        ROOT_URLCONF=__name__,
        ALLOWED_HOSTS=["*"],
        MIDDLEWARE=[],
        INSTALLED_APPS=[],
    )


urlpatterns = [path("", hello_world)]


if __name__ == "__main__":
    args = sys.argv
    if len(args) == 1:
        args = ["server.py", "runserver", "127.0.0.1:8000", "--noreload"]
    execute_from_command_line(args)
PY

yarn install
yarn workspace django-workspace python -c 'import django; import django.conf; print(django.get_version())'

LOG_FILE="${WORKDIR}/django-server.log"
yarn workspace django-workspace start > "${LOG_FILE}" 2>&1 &
SERVER_PID="$!"

for _ in {1..30}; do
  if curl -fsS http://127.0.0.1:8000/ > "${WORKDIR}/response.txt"; then
    break
  fi
  sleep 1
done

grep -F "Hello world from Django via pypi protocol!" "${WORKDIR}/response.txt"
