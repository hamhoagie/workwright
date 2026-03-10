"""
Evaluator — the two questions.

1. Why are we making this? (Does it serve the purpose?)
2. How does it solve it elegantly? (Nothing extra, nothing missing?)

Enforces Unix principles: single responsibility, readable, concise.
"""

import ast
from pathlib import Path
from dataclasses import dataclass
from typing import Optional


@dataclass
class Evaluation:
    """Result of evaluating a unit of work."""
    path: str
    score: float                    # 0.0 to 1.0
    single_responsibility: bool
    readable: bool
    concise: bool
    issues: list[str]
    suggestion: Optional[str]


# Thresholds
MAX_FUNC_LINES = 30
MAX_FUNC_ARGS = 5
MAX_FILE_FUNCS = 10
MAX_FILE_LINES = 300
MAX_NESTING = 5


def evaluate_file(path: str | Path) -> Evaluation:
    """Evaluate a single file against Unix principles."""
    path = Path(path)
    if not path.exists():
        return Evaluation(
            path=str(path), score=0.0,
            single_responsibility=False, readable=False, concise=False,
            issues=["File does not exist"], suggestion=None,
        )

    content = path.read_text()
    issues = []

    concise = _check_concise(content, issues)
    single_responsibility, readable = _check_python(path, content, issues)

    checks = [single_responsibility, readable, concise]
    score = max(0.0, sum(checks) / len(checks) - len(issues) * 0.05)

    return Evaluation(
        path=str(path),
        score=round(score, 2),
        single_responsibility=single_responsibility,
        readable=readable,
        concise=concise,
        issues=issues,
        suggestion=_suggest(issues) if issues else None,
    )


def _check_concise(content: str, issues: list[str]) -> bool:
    """Is the file concise?"""
    lines = len(content.split("\n"))
    if lines > MAX_FILE_LINES:
        issues.append(f"File is {lines} lines (max {MAX_FILE_LINES})")
        return False
    return True


def _check_python(path: Path, content: str, issues: list[str]) -> tuple[bool, bool]:
    """Check Python-specific principles. Returns (single_responsibility, readable)."""
    if path.suffix != ".py":
        return True, True

    try:
        tree = ast.parse(content)
    except SyntaxError as e:
        issues.append(f"Syntax error: {e}")
        return True, False

    single = _check_file_responsibility(tree, issues)
    readable = _check_functions(tree, issues)
    return single, readable


def _check_file_responsibility(tree: ast.Module, issues: list[str]) -> bool:
    """Does the file have a single responsibility?"""
    top_funcs = [n for n in ast.iter_child_nodes(tree)
                 if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]
    if len(top_funcs) > MAX_FILE_FUNCS:
        issues.append(f"{len(top_funcs)} top-level functions (max {MAX_FILE_FUNCS})")
        return False
    return True


def _check_functions(tree: ast.Module, issues: list[str]) -> bool:
    """Are individual functions readable?"""
    readable = True
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            if _check_one_function(node, issues):
                readable = False
    return readable


def _check_one_function(node: ast.FunctionDef, issues: list[str]) -> bool:
    """Check one function. Returns True if issues found."""
    name = node.name
    found_issues = False

    # Length
    if hasattr(node, "end_lineno") and node.end_lineno:
        length = node.end_lineno - node.lineno + 1
        if length > MAX_FUNC_LINES:
            issues.append(f"{name}() is {length} lines (max {MAX_FUNC_LINES})")
            found_issues = True

    # Arg count
    arg_count = _count_args(node.args)
    if arg_count > MAX_FUNC_ARGS:
        issues.append(f"{name}() has {arg_count} args (max {MAX_FUNC_ARGS})")
        found_issues = True

    # Nesting
    depth = _max_nesting(node)
    if depth > MAX_NESTING:
        issues.append(f"{name}() nests {depth} deep (max {MAX_NESTING})")
        found_issues = True

    return found_issues


def _count_args(args: ast.arguments) -> int:
    """Count function arguments, excluding self/cls."""
    count = len(args.args) + len(args.kwonlyargs)
    if args.vararg:
        count += 1
    if args.kwarg:
        count += 1
    if args.args and args.args[0].arg in ("self", "cls"):
        count -= 1
    return count


def _max_nesting(node: ast.AST, depth: int = 0) -> int:
    """Calculate maximum nesting depth."""
    max_d = depth
    nesting_types = (ast.If, ast.For, ast.While, ast.With, ast.Try, ast.ExceptHandler)
    for child in ast.iter_child_nodes(node):
        next_depth = depth + 1 if isinstance(child, nesting_types) else depth
        max_d = max(max_d, _max_nesting(child, next_depth))
    return max_d


def _suggest(issues: list[str]) -> str:
    """Generate a suggestion from issues."""
    suggestions = {
        "top-level functions": "Extract related functions into their own module.",
        "args": "Consider a config object or breaking the function into steps.",
        "nests": "Flatten with early returns or extract inner logic.",
        "lines": "Extract logic into helper functions.",
    }
    for keyword, suggestion in suggestions.items():
        if any(keyword in i for i in issues):
            return suggestion
    return "Simplify."
