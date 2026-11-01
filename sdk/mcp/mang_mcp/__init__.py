"""mang-mcp——mang-shell 编辑器的 MCP server。由 mang_mcp.server 提供全部实现。"""

from .server import main, mcp

__version__ = "0.1.0"

__all__ = ["mcp", "main", "__version__"]
