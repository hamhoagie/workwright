"""
Evaluator — the two questions.

1. Why are we making this? (Does it serve the purpose?)
2. How does it solve it elegantly? (Nothing extra, nothing missing?)

Also enforces Unix principles:
- Does this file do one thing?
- Does this function do one thing?
- Is it readable?
- Is it concise?
"""

import ast
import re
from pathlib import Path
from dataclasses import dataclass
from typing import Optional


@dataclass
class Evaluation:
    """Result of evaluating a unit of work."""
    path: str
    score: float                    # 0.0 to 1.0
    single_responsibility: bool     # does the file/function do one thing?
    readable: bool                  # is it clear?
    concise: bool                   # nothing unnecessary?
    issues: list[str]               # specific problems found
    suggestion: Optional[str]       # how to improve


class Evaluator:
    """
    Automated evaluation against Unix principles.

    This is the machine part. The human taste layer sits on top.
    Together they answer the two questions.
    """

    # Thresholds (tunable)
    MAX_FUNCTION_LINES = 30         # a function doing one thing fits in 30 lines
    MAX_FUNCTION_ARGS = 5           # more args = probably doing too much
    MAX_FILE_FUNCTIONS = 10         # more functions = file does too many things
    MAX_FILE_LINES = 300            # concise files
    MAX_COMPLEXITY = 5              # nesting depth

    def evaluate_file(self, path: str | Path) -> Evaluation:
        """Evaluate a single file against Unix principles."""
        path = Path(path)
        if not path.exists():
            return Evaluation(
                path=str(path), score=0.0,
                single_responsibility=False, readable=False, concise=False,
                issues=["File does not exist"], suggestion=None
            )

        content = path.read_text()
        issues = []

        # --- Concise ---
        lines = content.split("\n")
        line_count = len(lines)
        concise = line_count <= self.MAX_FILE_LINES
        if not concise:
            issues.append(f"File is {line_count} lines (max {self.MAX_FILE_LINES})")

        # --- Single responsibility (Python files) ---
        single_responsibility = True
        readable = True

        if path.suffix == ".py":
            try:
                tree = ast.parse(content)
                functions = [n for n in ast.walk(tree)
                           if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]
                classes = [n for n in ast.walk(tree) if isinstance(n, ast.ClassDef)]

                # Too many top-level functions = too many responsibilities
                top_funcs = [n for n in ast.iter_child_nodes(tree)
                           if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]
                if len(top_funcs) > self.MAX_FILE_FUNCTIONS:
                    single_responsibility = False
                    issues.append(
                        f"{len(top_funcs)} top-level functions "
                        f"(max {self.MAX_FILE_FUNCTIONS})"
                    )

                # Check individual functions
                for func in functions:
                    func_issues = self._evaluate_function(func)
                    if func_issues:
                        readable = False
                        issues.extend(func_issues)

            except SyntaxError as e:
                issues.append(f"Syntax error: {e}")
                readable = False

        # --- Score ---
        checks = [single_responsibility, readable, concise]
        passing = sum(checks)
        score = passing / len(checks)

        # Penalty for issues
        score = max(0.0, score - (len(issues) * 0.05))

        suggestion = None
        if issues:
            suggestion = self._suggest(issues)

        return Evaluation(
            path=str(path),
            score=round(score, 2),
            single_responsibility=single_responsibility,
            readable=readable,
            concise=concise,
            issues=issues,
            suggestion=suggestion,
        )

    def _evaluate_function(self, node: ast.FunctionDef) -> list[str]:
        """Check a single function against Unix principles."""
        issues = []
        name = node.name

        # Line count
        if hasattr(node, "end_lineno") and node.end_lineno:
            length = node.end_lineno - node.lineno + 1
            if length > self.MAX_FUNCTION_LINES:
                issues.append(
                    f"{name}() is {length} lines (max {self.MAX_FUNCTION_LINES})"
                )

        # Argument count
        args = node.args
        arg_count = len(args.args) + len(args.kwonlyargs)
        if args.vararg:
            arg_count += 1
        if args.kwarg:
            arg_count += 1
        # Don't count 'self' or 'cls'
        if args.args and args.args[0].arg in ("self", "cls"):
            arg_count -= 1

        if arg_count > self.MAX_FUNCTION_ARGS:
            issues.append(
                f"{name}() has {arg_count} args (max {self.MAX_FUNCTION_ARGS})"
            )

        # Nesting depth
        max_depth = self._max_nesting(node)
        if max_depth > self.MAX_COMPLEXITY:
            issues.append(
                f"{name}() nests {max_depth} deep (max {self.MAX_COMPLEXITY})"
            )

        return issues

    def _max_nesting(self, node: ast.AST, depth: int = 0) -> int:
        """Calculate maximum nesting depth."""
        max_d = depth
        for child in ast.iter_child_nodes(node):
            if isinstance(child, (ast.If, ast.For, ast.While, ast.With,
                                  ast.Try, ast.ExceptHandler)):
                max_d = max(max_d, self._max_nesting(child, depth + 1))
            else:
                max_d = max(max_d, self._max_nesting(child, depth))
        return max_d

    def _suggest(self, issues: list[str]) -> str:
        """Generate a simple suggestion from issues."""
        if any("lines" in i and "function" not in i.lower() for i in issues):
            return "Split into smaller files, each with a single responsibility."
        if any("functions" in i for i in issues):
            return "Extract related functions into their own module."
        if any("args" in i for i in issues):
            return "Consider a config object or breaking the function into steps."
        if any("nests" in i for i in issues):
            return "Flatten with early returns or extract inner logic."
        if any("lines" in i for i in issues):
            return "Extract logic into helper functions."
        return "Simplify."
