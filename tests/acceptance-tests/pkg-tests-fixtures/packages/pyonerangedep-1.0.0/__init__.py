import json
import pathlib

import pynodeps

_manifest_path = pathlib.Path(__file__).with_name("package.json")
_manifest = json.loads(_manifest_path.read_text())

name = _manifest["name"]
version = _manifest["version"]


def to_dict():
    return {
        "name": name,
        "version": version,
        "dependencies": {
            "pynodeps": pynodeps.to_dict(),
        },
    }
