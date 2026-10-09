// Progressive enhancement for the shared .dtable table component.
//
// Reads the per-column narrow-screen behaviour declared on each
// <th data-stack="primary|merge|hide"> and stamps every body cell in that
// column with the matching class (and, for labelled columns, a data-label
// taken from the header). Once a table is processed it gets
// .dtable--stack-ready, which is what unlocks the stacked-card layout in the
// stylesheet's narrow-screen media query.
//
// This is purely additive: if this script never runs, a .dtable--stack table
// keeps its default behaviour and simply scrolls horizontally on narrow
// screens. Nothing here is required for a table to be usable.
(function () {
    "use strict";

    var CLASS = {
        primary: "dtable__cell--primary",
        merge: "dtable__cell--merge",
        hide: "dtable__cell--hide",
        label: "dtable__cell--label"
    };

    function columnMeta(table) {
        var headRow = table.tHead && table.tHead.rows[0];
        if (!headRow) return null;
        var meta = [];
        for (var i = 0; i < headRow.cells.length; i++) {
            var th = headRow.cells[i];
            var kind = th.getAttribute("data-stack") || "label";
            if (!CLASS[kind]) kind = "label";
            meta.push({
                kind: kind,
                label: (th.getAttribute("data-stack-label") || th.textContent || "").trim()
            });
        }
        return meta;
    }

    function stampTable(table) {
        var meta = columnMeta(table);
        if (!meta) return;
        for (var b = 0; b < table.tBodies.length; b++) {
            var rows = table.tBodies[b].rows;
            for (var r = 0; r < rows.length; r++) {
                var row = rows[r];
                if (row.classList.contains("dtable__group")) continue;
                for (var c = 0; c < row.cells.length && c < meta.length; c++) {
                    var cell = row.cells[c];
                    var col = meta[c];
                    cell.classList.add(CLASS[col.kind]);
                    // Labelled columns need the header text for the "Label: value"
                    // prefix; don't clobber a data-label the template set itself.
                    if (col.kind === "label" && !cell.hasAttribute("data-label")) {
                        cell.setAttribute("data-label", col.label);
                    }
                }
            }
        }
        table.classList.add("dtable--stack-ready");
    }

    function enhance() {
        var tables = document.querySelectorAll("table.dtable--stack");
        for (var i = 0; i < tables.length; i++) {
            stampTable(tables[i]);
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", enhance);
    } else {
        enhance();
    }
})();
