"""文件端口语义。

只判断文件是否存在，不解析内容。
base_dir 在这里注入（不是 loader）；同一份 graph 对象可被不同实例用不同 base_dir 跑。
"""

from pathlib import Path

from .node import Node


class PortPathError(Exception):
    """端口文件路径解析失败（越界 / 类型错误）。"""


def iport_satisfied(node: Node, base_dir: Path) -> tuple[int, ...]:
    """返回当前满足条件的 iport 组索引（OR-of-AND）。

    组内 AND：组内所有文件都存在，组才满足。
    组间 OR：任一组满足即可。
    空 tuple 表示无输入就绪。
    """
    return _satisfied_group_indices(node.iport, base_dir)


def oport_produced(node: Node, base_dir: Path) -> tuple[int, ...]:
    """返回已产出的 oport 组索引（OR）。

    组内 AND：组内所有文件都存在，组才产出。
    组间 OR：任一组产出即可。
    """
    return _satisfied_group_indices(node.oport, base_dir)


def _satisfied_group_indices(
    groups: tuple[tuple[str, ...], ...], base_dir: Path
) -> tuple[int, ...]:
    out: list[int] = []
    for idx, group in enumerate(groups):
        if not group:
            # 空组视为不满足（既非 AND 也非 OR 的有效输入）
            continue
        all_exist = True
        for spec in group:
            try:
                p = port_file_path(spec, base_dir)
            except PortPathError:
                all_exist = False
                break
            if not p.exists():
                all_exist = False
                break
        if all_exist:
            out.append(idx)
    return tuple(out)


def port_file_path(spec: str, base_dir: Path) -> Path:
    """解析 "doc/spec.md" -> <base_dir>/doc/spec.md。

    - 路径必须为相对路径（不以 / 开头）；绝对路径抛 PortPathError。
    - 解析后必须仍在 base_dir 之下（防御 ../ 越界），否则抛 PortPathError。
    """
    if not isinstance(spec, str) or not spec:
        raise PortPathError(f"port spec 必须是非空字符串，实际: {spec!r}")
    if Path(spec).is_absolute():
        raise PortPathError(f"port spec 必须是相对路径: {spec!r}")

    base = base_dir.resolve()
    target = (base / spec).resolve()
    try:
        target.relative_to(base)
    except ValueError as exc:
        raise PortPathError(
            f"port spec 「{spec}」 解析后逃出 base_dir 「{base_dir}」"
        ) from exc
    return target


def expand_iport_paths(node: Node, base_dir: Path) -> tuple[tuple[Path, ...], ...]:
    """展开 iport 所有路径，便于监控器列出节点在等哪些文件。

    返回与 node.iport 同形状的二维 tuple，元素为 Path（不检查存在性）。
    """
    return _resolve_port_groups(node.iport, base_dir)


def expand_oport_paths(node: Node, base_dir: Path) -> tuple[tuple[Path, ...], ...]:
    """展开 oport 所有路径，便于监控器列出节点会产生哪些文件。"""
    return _resolve_port_groups(node.oport, base_dir)


def _resolve_port_groups(
    groups: tuple[tuple[str, ...], ...], base_dir: Path
) -> tuple[tuple[Path, ...], ...]:
    out: list[tuple[Path, ...]] = []
    for group in groups:
        expanded: list[Path] = []
        for spec in group:
            expanded.append(port_file_path(spec, base_dir))
        out.append(tuple(expanded))
    return tuple(out)