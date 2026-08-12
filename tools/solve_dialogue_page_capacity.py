#!/usr/bin/env python3
"""Solve bounded dialogue workset-to-font-page union capacity with SciPy/HiGHS."""

from __future__ import annotations

import json
import sys

import numpy as np
import scipy
from scipy.optimize import Bounds, LinearConstraint, milp
from scipy.sparse import coo_array


def main() -> int:
    request = json.load(sys.stdin)
    worksets = request["worksets"]
    page_count = int(request["maximum_page_count"])
    capacity = int(request["active_slot_count"])
    timeout_seconds = float(request["timeout_seconds"])
    workset_count = len(worksets)
    glyph_count = 1 + max(
        glyph
        for workset in worksets
        for glyph in workset["glyph_indices"]
    )
    preserved_codes = sorted(
        {
            code
            for workset in worksets
            for code in workset["preserved_codes"]
        }
    )
    preserved_index = {code: index for index, code in enumerate(preserved_codes)}

    assignment_count = workset_count * page_count
    glyph_presence_offset = assignment_count
    glyph_presence_count = glyph_count * page_count
    code_presence_offset = glyph_presence_offset + glyph_presence_count
    code_presence_count = len(preserved_codes) * page_count
    variable_count = code_presence_offset + code_presence_count

    def assignment(workset: int, page: int) -> int:
        return workset * page_count + page

    def glyph_presence(glyph: int, page: int) -> int:
        return glyph_presence_offset + glyph * page_count + page

    def code_presence(code: int, page: int) -> int:
        return code_presence_offset + preserved_index[code] * page_count + page

    rows: list[int] = []
    columns: list[int] = []
    values: list[float] = []
    lower_bounds: list[float] = []
    upper_bounds: list[float] = []

    def add_constraint(
        terms: list[tuple[int, float]], lower: float, upper: float
    ) -> None:
        row = len(lower_bounds)
        for column, value in terms:
            rows.append(row)
            columns.append(column)
            values.append(value)
        lower_bounds.append(lower)
        upper_bounds.append(upper)

    for workset in range(workset_count):
        add_constraint(
            [(assignment(workset, page), 1.0) for page in range(page_count)],
            1.0,
            1.0,
        )
    add_constraint([(assignment(0, 0), 1.0)], 1.0, 1.0)

    for workset, demand in enumerate(worksets):
        for page in range(page_count):
            assigned = assignment(workset, page)
            for glyph in demand["glyph_indices"]:
                add_constraint(
                    [(assigned, 1.0), (glyph_presence(glyph, page), -1.0)],
                    -np.inf,
                    0.0,
                )
            for code in demand["preserved_codes"]:
                add_constraint(
                    [(assigned, 1.0), (code_presence(code, page), -1.0)],
                    -np.inf,
                    0.0,
                )

    for page in range(page_count):
        add_constraint(
            [
                (glyph_presence(glyph, page), 1.0)
                for glyph in range(glyph_count)
            ]
            + [
                (code_presence(code, page), 1.0)
                for code in preserved_codes
            ],
            -np.inf,
            float(capacity),
        )
    for page in range(page_count - 1):
        add_constraint(
            [(assignment(workset, page), 1.0) for workset in range(workset_count)]
            + [
                (assignment(workset, page + 1), -1.0)
                for workset in range(workset_count)
            ],
            0.0,
            np.inf,
        )

    objective = np.zeros(variable_count)
    objective[glyph_presence_offset:] = 1.0
    integrality = np.zeros(variable_count, dtype=np.uint8)
    integrality[:assignment_count] = 1
    matrix = coo_array(
        (np.asarray(values), (np.asarray(rows), np.asarray(columns))),
        shape=(len(lower_bounds), variable_count),
    ).tocsr()
    result = milp(
        objective,
        integrality=integrality,
        bounds=Bounds(np.zeros(variable_count), np.ones(variable_count)),
        constraints=LinearConstraint(
            matrix,
            np.asarray(lower_bounds),
            np.asarray(upper_bounds),
        ),
        options={"disp": False, "time_limit": timeout_seconds},
    )

    page_indices = None
    if result.x is not None:
        candidate = []
        valid = True
        for workset in range(workset_count):
            values_for_workset = result.x[
                assignment(workset, 0) : assignment(workset, 0) + page_count
            ]
            page = int(np.argmax(values_for_workset))
            if values_for_workset[page] < 0.5:
                valid = False
                break
            candidate.append(page)
        if valid:
            page_indices = canonicalize_pages(candidate)

    if page_indices is not None:
        status = "feasible"
    elif result.status == 2:
        status = "infeasible"
    elif result.status == 1:
        status = "timeout"
    else:
        status = "error"
    json.dump(
        {
            "status": status,
            "solver_version": f"SciPy {scipy.__version__} HiGHS",
            "message": str(result.message),
            "page_indices": page_indices,
        },
        sys.stdout,
        separators=(",", ":"),
    )
    sys.stdout.write("\n")
    return 0


def canonicalize_pages(page_indices: list[int]) -> list[int]:
    canonical: dict[int, int] = {}
    result = []
    for page in page_indices:
        if page not in canonical:
            canonical[page] = len(canonical)
        result.append(canonical[page])
    return result


if __name__ == "__main__":
    raise SystemExit(main())
