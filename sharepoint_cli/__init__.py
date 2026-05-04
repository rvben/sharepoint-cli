"""sharepoint-cli: Agent-friendly SharePoint Online CLI."""

try:
    from importlib.metadata import version
    __version__ = version("sharepoint-cli")
except ImportError:
    from importlib_metadata import version
    __version__ = version("sharepoint-cli")
