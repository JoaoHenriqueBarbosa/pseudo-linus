// Progressive enhancement: rewrite opted-in <time> elements as relative
// dates ("3 days ago"), keeping the exact date visible on hover.
//
// Opt-in markup (nothing happens without the class):
//   <time class="relative-time" datetime="2026-07-08">2026-07-08</time>
//   <time class="relative-time" datetime="2026-07-08T10:23:58-07:00">…</time>
//
// The element's text is replaced with a relative phrasing of its
// datetime attribute, and the original text moves to the title
// attribute so hovering shows the absolute date. Purely additive: if
// this script never runs, or a value cannot be parsed, or the browser
// lacks Intl.RelativeTimeFormat, the absolute date simply remains.
(function () {
    "use strict";

    if (typeof Intl === "undefined" || !Intl.RelativeTimeFormat) return;

    var format = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
    var DATE_ONLY = /^(\d{4})-(\d{2})-(\d{2})$/;
    var MS_PER_DAY = 24 * 60 * 60 * 1000;

    function parse(value) {
        var dateOnly = DATE_ONLY.exec(value);
        if (dateOnly) {
            // parse as local midnight so a calendar date is not shifted
            // by the viewer's UTC offset
            return {
                date: new Date(+dateOnly[1], dateOnly[2] - 1, +dateOnly[3]),
                hasTime: false
            };
        }
        var date = new Date(value);
        return isNaN(date.getTime()) ? null : { date: date, hasTime: true };
    }

    function phrase(parsed) {
        var now = new Date();

        if (parsed.hasTime) {
            var seconds = Math.round((parsed.date - now) / 1000);
            if (Math.abs(seconds) < 60) return format.format(seconds, "second");
            var minutes = Math.round(seconds / 60);
            if (Math.abs(minutes) < 60) return format.format(minutes, "minute");
            var hours = Math.round(minutes / 60);
            if (Math.abs(hours) < 24) return format.format(hours, "hour");
        }

        // calendar-day difference, so "yesterday" flips at midnight
        var today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
        var day = new Date(parsed.date.getFullYear(), parsed.date.getMonth(), parsed.date.getDate());
        var days = Math.round((day - today) / MS_PER_DAY);
        if (Math.abs(days) < 7) return format.format(days, "day");
        if (Math.abs(days) < 31) return format.format(Math.trunc(days / 7), "week");
        if (Math.abs(days) < 365) return format.format(Math.trunc(days / 30.44), "month");
        return format.format(Math.trunc(days / 365.25), "year");
    }

    function enhance() {
        var elements = document.querySelectorAll("time.relative-time[datetime]");
        for (var i = 0; i < elements.length; i++) {
            var element = elements[i];
            var parsed = parse(element.getAttribute("datetime"));
            if (!parsed) continue;
            if (!element.title) element.title = element.textContent.trim();
            element.textContent = phrase(parsed);
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", enhance);
    } else {
        enhance();
    }
})();
