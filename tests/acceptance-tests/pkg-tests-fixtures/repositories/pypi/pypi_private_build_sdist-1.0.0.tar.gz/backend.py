from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile


def get_requires_for_build_wheel(config_settings=None):
    return ["pypi-no-deps==1.1.0"]


def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    import pypi_no_deps

    if pypi_no_deps.VALUE != "1.1.0":
        raise RuntimeError("dynamic build requirements were not installed")

    wheel_name = "pypi_private_build_sdist-1.0.0-py3-none-any.whl"
    output_path = Path(wheel_directory) / wheel_name

    files = {
        "pypi_private_build_sdist/__init__.py": "VALUE = 'private-build-requirement'\n",
        "pypi_private_build_sdist-1.0.0.dist-info/METADATA": "\n".join([
            "Metadata-Version: 2.1",
            "Name: pypi-private-build-sdist",
            "Version: 1.0.0",
            "",
        ]),
        "pypi_private_build_sdist-1.0.0.dist-info/WHEEL": "\n".join([
            "Wheel-Version: 1.0",
            "Generator: zpm-test-backend",
            "Root-Is-Purelib: true",
            "Tag: py3-none-any",
            "",
        ]),
        "pypi_private_build_sdist-1.0.0.dist-info/RECORD": "",
    }

    with ZipFile(output_path, "w", ZIP_DEFLATED) as wheel:
        for name, content in files.items():
            wheel.writestr(name, content)

    return wheel_name
