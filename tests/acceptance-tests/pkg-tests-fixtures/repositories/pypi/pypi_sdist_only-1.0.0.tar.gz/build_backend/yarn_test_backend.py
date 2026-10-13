# A minimal PEP 517 backend. It generates `pypi_sdist_only/_built.py`, which
# doesn't exist in the sources, so that the tests can tell that the wheel
# went through the build.

import base64, hashlib, os, zipfile

NAME = "pypi_sdist_only"
VERSION = "1.0.0"
DIST_INFO = f"{NAME}-{VERSION}.dist-info"

def get_requires_for_build_wheel(config_settings=None):
    return []

def _record_line(path, data):
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"{path},sha256={digest},{len(data)}"

def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    files = {}

    for name in sorted(os.listdir(NAME)):
        if name.endswith(".py"):
            with open(os.path.join(NAME, name), "rb") as f:
                files[f"{NAME}/{name}"] = f.read()

    files[f"{NAME}/_built.py"] = b"BUILT_BY = 'yarn-test-backend'\n"

    with open("PKG-INFO", "rb") as f:
        files[f"{DIST_INFO}/METADATA"] = f.read()

    files[f"{DIST_INFO}/WHEEL"] = b"Wheel-Version: 1.0\nGenerator: yarn-test-backend\nRoot-Is-Purelib: true\nTag: py3-none-any\n"

    record = [_record_line(path, data) for path, data in files.items()]
    record.append(f"{DIST_INFO}/RECORD,,")
    files[f"{DIST_INFO}/RECORD"] = ("\n".join(record) + "\n").encode()

    wheel_name = f"{NAME}-{VERSION}-py3-none-any.whl"

    with zipfile.ZipFile(os.path.join(wheel_directory, wheel_name), "w") as wheel:
        for path, data in files.items():
            wheel.writestr(path, data)

    return wheel_name
