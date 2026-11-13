// Two charts: the real validator set, and the simulated supply. All labels go in through
// textContent, because validator names are written by strangers.

const SVG = "http://www.w3.org/2000/svg";
const INK = { muted: "#7d7364", grid: "#e7dcc7", base: "#c9bca3" };

function node(name, attrs = {}, parent) {
  const element = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attrs)) element.setAttribute(key, value);
  if (parent) parent.appendChild(element);
  return element;
}

function label(parent, x, y, text, attrs = {}) {
  const t = node("text", { x, y, "font-size": 12, fill: INK.muted, ...attrs }, parent);
  t.textContent = text;
  return t;
}

function tooltipFor(root) {
  let tip = root.querySelector(":scope > .tip");
  if (!tip) {
    tip = document.createElement("div");
    tip.className = "tip";
    tip.hidden = true;
    root.appendChild(tip);
  }
  return tip;
}

function placeTip(root, tip, clientX, clientY) {
  const box = root.getBoundingClientRect();
  tip.hidden = false;
  const x = clientX - box.left;
  const y = clientY - box.top;
  const flip = x > box.width - tip.offsetWidth - 24;
  tip.style.left = `${flip ? x - tip.offsetWidth - 14 : x + 14}px`;
  tip.style.top = `${Math.max(0, Math.min(y - 12, box.height - tip.offsetHeight))}px`;
}

function columnPath(x, top, width, bottom) {
  const r = Math.min(4, width / 2);
  if (width < 6) return `M${x} ${bottom}V${top}H${x + width}V${bottom}Z`;
  return `M${x} ${bottom}V${top + r}Q${x} ${top} ${x + r} ${top}H${x + width - r}Q${x + width} ${top} ${x + width} ${top + r}V${bottom}Z`;
}

const clean = (max) => {
  for (const step of [1, 2, 5, 10, 20, 25, 50, 100]) if (max / step <= 5) return step;
  return 100;
};

/**
 * Every validator, biggest to smallest, on one linear scale. Its job is one picture: a few huge
 * bars and a long row of tiny ones. Rows: { id, share, kind: "big" | "runt" | "other", detail }.
 */
export function drawLitter(root, rows, nakamoto, haltingShare) {
  const svgHost = root.querySelector(".plot");
  const tip = tooltipFor(root);
  const width = svgHost.clientWidth || 900;
  const height = 240;
  const m = { left: 44, right: 6, top: 30, bottom: 26 };
  const plotW = width - m.left - m.right;
  const plotH = height - m.top - m.bottom;
  const peak = rows[0].share * 100;
  const step = clean(peak);
  const yMax = Math.ceil(peak / step) * step;
  const y = (percent) => m.top + plotH - (percent / yMax) * plotH;

  svgHost.textContent = "";
  const svg = node("svg", { viewBox: `0 0 ${width} ${height}`, width, height, role: "img", "aria-label": "Stake held by each validator, biggest to smallest" }, svgHost);
  for (let tick = 0; tick <= yMax; tick += step) {
    node("line", { x1: m.left, x2: width - m.right, y1: y(tick), y2: y(tick), stroke: tick === 0 ? INK.base : INK.grid }, svg);
    label(svg, m.left - 8, y(tick) + 4, `${tick}%`, { "text-anchor": "end" });
  }
  const slot = plotW / rows.length;
  const barW = Math.max(1, Math.min(24, slot - 2));
  rows.forEach((row, i) => {
    const x = m.left + i * slot + (slot - barW) / 2;
    const percent = row.share * 100;
    const top = Math.min(y(percent), y(0) - 2);
    node("path", { d: columnPath(x, top, barW, y(0)), class: `bar ${row.kind}` }, svg);
    const hit = node("rect", { x: m.left + i * slot, y: m.top, width: slot, height: plotH, fill: "transparent", class: "hit" }, svg);
    const show = (event) => {
      tip.textContent = "";
      const strong = document.createElement("strong");
      strong.textContent = `${percent < 0.01 ? percent.toFixed(4) : percent.toFixed(2)}% of stake`;
      const name = document.createElement("div");
      name.textContent = row.id;
      const more = document.createElement("div");
      more.className = "soft";
      more.textContent = row.detail;
      tip.append(strong, name, more);
      placeTip(root, tip, event.clientX, event.clientY);
    };
    hit.addEventListener("pointermove", show);
    hit.addEventListener("pointerleave", () => (tip.hidden = true));
  });
  // The bracket says the one thing the chart is for.
  const bracketRight = m.left + nakamoto * slot - 1;
  node("path", { d: `M${m.left + 1} ${m.top - 6}V${m.top - 12}H${bracketRight}V${m.top - 6}`, fill: "none", stroke: "#1d1a16", "stroke-width": 1 }, svg);
  label(svg, m.left + 2, m.top - 17, `These ${nakamoto} hold ${(haltingShare * 100).toFixed(1)}% of all stake`, { fill: "#1d1a16", "font-weight": 600, "font-size": 13 });
  label(svg, width - m.right, height - 6, `smallest of ${rows.length}`, { "text-anchor": "end" });
  label(svg, m.left, height - 6, "biggest");
}

const SHOWN = [
  ["exodus", "#1a8f63"],
  ["steady", "#2a6fc4"],
  ["herd", "#d98a00"],
  ["ghost", "#c23a55"],
];

/** Supply by day. Four scenarios carry colour and an end label, and the rest stay as grey context. */
export function drawSupply(root, data) {
  const svgHost = root.querySelector(".plot");
  const tip = tooltipFor(root);
  const width = svgHost.clientWidth || 900;
  const height = 300;
  const m = { left: 50, right: 215, top: 12, bottom: 28 };
  const plotW = width - m.left - m.right;
  const plotH = height - m.top - m.bottom;
  const days = 60;
  const yMax = 60;
  const x = (day) => m.left + ((day - 1) / (days - 1)) * plotW;
  const y = (value) => m.top + plotH - (Math.min(value, yMax) / yMax) * plotH;
  const colour = Object.fromEntries(SHOWN);
  const keys = Object.keys(data.scenarios);

  svgHost.textContent = "";
  const svg = node("svg", { viewBox: `0 0 ${width} ${height}`, width, height, role: "img", "aria-label": "Simulated RUNT supply over 60 days in each scenario" }, svgHost);
  for (let tick = 0; tick <= yMax; tick += 20) {
    node("line", { x1: m.left, x2: width - m.right, y1: y(tick), y2: y(tick), stroke: tick === 0 ? INK.base : INK.grid }, svg);
    label(svg, m.left - 8, y(tick) + 4, tick === 0 ? "0" : `${tick}M`, { "text-anchor": "end" });
  }
  for (const day of [1, 10, 20, 30, 40, 50, 60]) label(svg, x(day), height - 8, `Day ${day}`, { "text-anchor": "middle" });

  const path = (values) => values.map((v, i) => `${i ? "L" : "M"}${x(i + 1).toFixed(1)} ${y(v).toFixed(1)}`).join("");
  for (const key of keys) {
    if (colour[key]) continue;
    node("path", { d: path(data.scenarios[key].supply), fill: "none", stroke: "#d4c8b1", "stroke-width": 1.5, "stroke-linejoin": "round" }, svg);
  }
  for (const [key, stroke] of SHOWN) {
    const series = data.scenarios[key].supply;
    node("path", { d: path(series), fill: "none", stroke, "stroke-width": 2, "stroke-linejoin": "round", "stroke-linecap": "round" }, svg);
    const last = series[series.length - 1];
    node("circle", { cx: x(days), cy: y(last), r: 4, fill: stroke, stroke: "#fffaf0", "stroke-width": 2 }, svg);
    label(svg, x(days) + 10, y(last) + 4, `${data.scenarios[key].label} ${last.toFixed(0)}M`, { fill: "#1d1a16", "font-size": 12.5 });
  }
  const cross = node("line", { y1: m.top, y2: m.top + plotH, stroke: INK.muted, "stroke-width": 1, visibility: "hidden" }, svg);
  const overlay = node("rect", { x: m.left, y: m.top, width: plotW, height: plotH, fill: "transparent" }, svg);
  overlay.addEventListener("pointermove", (event) => {
    const box = svg.getBoundingClientRect();
    const day = Math.max(1, Math.min(days, Math.round(1 + ((event.clientX - box.left - m.left) / plotW) * (days - 1))));
    cross.setAttribute("x1", x(day));
    cross.setAttribute("x2", x(day));
    cross.setAttribute("visibility", "visible");
    tip.textContent = "";
    const head = document.createElement("strong");
    head.textContent = `Day ${day}`;
    tip.appendChild(head);
    keys
      .map((key) => [key, data.scenarios[key].supply[day - 1]])
      .sort((a, b) => b[1] - a[1])
      .forEach(([key, value]) => {
        const row = document.createElement("div");
        row.className = "tip-row";
        const swatch = document.createElement("i");
        swatch.style.background = colour[key] ?? "#d4c8b1";
        const number = document.createElement("b");
        number.textContent = `${value.toFixed(1)}M`;
        const name = document.createElement("span");
        name.textContent = data.scenarios[key].label;
        row.append(swatch, number, name);
        tip.appendChild(row);
      });
    placeTip(root, tip, event.clientX, event.clientY);
  });
  overlay.addEventListener("pointerleave", () => {
    tip.hidden = true;
    cross.setAttribute("visibility", "hidden");
  });

  const legend = root.querySelector(".legend");
  legend.textContent = "";
  for (const [key, stroke] of [...SHOWN, ["", "#d4c8b1"]]) {
    const item = document.createElement("span");
    const line = document.createElement("i");
    line.style.background = stroke;
    item.append(line, document.createTextNode(key ? data.scenarios[key].label : "Other scenarios"));
    legend.appendChild(item);
  }
}
