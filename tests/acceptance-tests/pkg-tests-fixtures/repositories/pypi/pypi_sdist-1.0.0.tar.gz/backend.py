from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile


def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    wheel_name = "pypi_sdist-1.0.0-py3-none-any.whl"
    source_root = Path(__file__).parent
    output_path = Path(wheel_directory) / wheel_name

    files = {
        "pypi_sdist/__init__.py": (source_root / "pypi_sdist" / "__init__.py").read_text(),
        "pypi_sdist-1.0.0.dist-info/METADATA": "\n".join([
            "Metadata-Version: 2.1",
            "Name: pypi-sdist",
            "Version: 1.0.0",
            "Requires-Dist: pypi-no-deps (==1.0.0)",
            "",
        ]),
        "pypi_sdist-1.0.0.dist-info/WHEEL": "\n".join([
            "Wheel-Version: 1.0",
            "Generator: zpm-test-backend",
            "Root-Is-Purelib: true",
            "Tag: py3-none-any",
            "",
        ]),
        "pypi_sdist-1.0.0.dist-info/RECORD": "",
    }

    with ZipFile(output_path, "w", ZIP_DEFLATED) as wheel:
        for name, content in files.items():
            wheel.writestr(name, content)

    return wheel_name
