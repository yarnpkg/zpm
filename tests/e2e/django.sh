#!/usr/bin/env bash

mkdir -p packages/django-workspace

cat > package.json <<'JSON'
{
  "name": "e2e-django-root",
  "private": true,
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

LOG_FILE="${TEMP_DIR}/django-server.log"
yarn workspace django-workspace start > "${LOG_FILE}" 2>&1 &
SERVER_PID="$!"

wait_for http://127.0.0.1:8000/

curl -fsS http://127.0.0.1:8000/ > "${TEMP_DIR}/response.txt"
grep -F "Hello world from Django via pypi protocol!" "${TEMP_DIR}/response.txt"
