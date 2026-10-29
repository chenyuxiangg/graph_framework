"""software_team SOP 的 worker 集合。

reference impl：planer_proc / archer_proc 注册占位函数（写一个 stub 文件）；
同时注册 check_output gate op（满足 = iport 文件全部存在）。
"""

from pathlib import Path

from src.kernel import register_op, iport_satisfied
from src.kernel.node import Node


def check_output(base_dir: Path, state, graph, gate) -> bool:
    """reference check_output：current_node 的 oport 文件全部产出 → True。

    作为 gate op 挂在 planer 的出边上：planer 还没产 spec.md 时返回 False，
    触发 planer_loop（gate_value=False）；spec.md 产出后返回 True，
    触发 planer_to_archer（gate_value=True）。
    """
    from src.kernel import oport_produced

    if state.current_node is None:
        return False
    node = graph.node_index.get(state.current_node)
    if node is None:
        return False
    return bool(oport_produced(node, base_dir))


def planer_proc(base_dir: Path, state, graph, node_name: str) -> tuple[str, ...]:
    """reference planer_proc：写一个 stub 文档到 base_dir/doc/。"""
    out = base_dir / "doc" / "specification.md"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("# specification (stub)\n", encoding="utf-8")
    return (str(out),)


def archer_proc(base_dir: Path, state, graph, node_name: str) -> tuple[str, ...]:
    """reference archer_proc：写两个 stub 文档到 base_dir/doc/。"""
    out1 = base_dir / "doc" / "architectural_design.md"
    out2 = base_dir / "doc" / "organizational_structure.md"
    out1.parent.mkdir(parents=True, exist_ok=True)
    out1.write_text("# architectural_design (stub)\n", encoding="utf-8")
    out2.write_text("# organizational_structure (stub)\n", encoding="utf-8")
    return (str(out1), str(out2))


def cmd_push_spec_to_archer(base_dir: Path, state, graph, edge) -> None:
    """reference cmd：planer → archer 时不做副作用，仅 placeholder。"""
    return None


def cmd_back(base_dir: Path, state, graph, edge) -> None:
    """reference cmd：planer_loop 时不做副作用。"""
    return None


register_op("check_output", check_output)
register_op("planer_proc", planer_proc)
register_op("archer_proc", archer_proc)
register_op("cmd_push_spec_to_archer", cmd_push_spec_to_archer)
register_op("cmd_back", cmd_back)