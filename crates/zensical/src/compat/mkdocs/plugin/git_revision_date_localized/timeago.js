// Copyright (c) 2025-2026 Zensical and contributors
// SPDX-License-Identifier: MIT

(() => {
  const units = [
    ["year", 31536000], ["month", 2592000], ["week", 604800],
    ["day", 86400], ["hour", 3600], ["minute", 60], ["second", 1]
  ];
  function update() {
    for (const element of document.querySelectorAll(".git-revision-date-localized-plugin .timeago")) {
      const seconds = (Date.parse(element.getAttribute("datetime")) - Date.now()) / 1000;
      if (!Number.isFinite(seconds)) continue;
      let formatter;
      try {
        formatter = new Intl.RelativeTimeFormat((element.getAttribute("locale") || "en").replaceAll("_", "-"));
      } catch {
        formatter = new Intl.RelativeTimeFormat("en");
      }
      const [unit, size] = units.find(([, size]) => Math.abs(seconds) >= size) || units[units.length - 1];
      element.textContent = formatter.format(Math.round(seconds / size), unit);
    }
  }
  update();
  if (typeof document$ !== "undefined") document$.subscribe(update);
  setInterval(update, 60000);
})();
