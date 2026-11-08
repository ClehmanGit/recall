// The background: columns of everyday files and chats drifting past, the way a
// library looks when you are searching hundreds of things at once. Every couple
// of seconds one card lights up, as if the search just found it.
// Built once per width; it holds still for people who ask for less motion.

(() => {
const MARK = { claude: "./assets/claude.svg", chatgpt: "./assets/openai.svg", gemini: "./assets/gemini.svg" };

const lines = (...w) => w.map((x) => `<div class="ln ${x}"></div>`).join("");
const BITS = [
  ["doc", "Lease renewal 2026.docx", "Rent stays the same if we <mark>renew by March 1</mark>."],
  ["chat", "claude", "Running toilet fix", ["It keeps running after every flush", "Check the <mark>flapper chain</mark> first"]],
  ["pdf", "Dentist intake form.pdf", "Next cleaning moved to <mark>Tuesday at 9:30</mark>."],
  ["pic", "a", "Lake sunset.jpg"],
  ["xls", "Monthly budget.xlsx"],
  ["txt", "Packing list, lake trip.md", "Sunscreen, <mark>bug spray</mark>, extra towels, chargers."],
  ["chat", "chatgpt", "Lemon chicken", ["What can I swap for capers?", "Green olives, <mark>chopped fine</mark>"]],
  ["doc", "Garden plan.docx", "Tomatoes along the <mark>south fence</mark> this year."],
  ["pdf", "Car insurance renewal.pdf", "Premium due <mark>June 14</mark>, autopay is on."],
  ["chat", "gemini", "Birthday ideas", ["Something for a 7 year old who loves space", "A <mark>planetarium night</mark>"]],
  ["pic", "b", "Hike, Pine Ridge.jpg"],
  ["txt", "Wifi and alarm codes.txt", "Guest network is <mark>on the fridge</mark> note."],
  ["xls", "Wedding guest list.xlsx"],
  ["doc", "Cover letter draft.docx", "I led the <mark>move to the new system</mark> in eight weeks."],
  ["chat", "claude", "Email to the landlord", ["Make this sound polite but firm", "Here's a <mark>shorter version</mark>"]],
  ["pdf", "Warranty, dishwasher.pdf", "Covered for <mark>two years</mark> from purchase."],
  ["pic", "c", "Nursery colors.png"],
  ["txt", "Book club picks.md", "November: <mark>the lighthouse one</mark> Sam liked."],
  ["chat", "chatgpt", "Tax questions", ["Can we deduct the home office?", "Only the <mark>part used for work</mark>"]],
  ["doc", "Meeting notes, school.docx", "Pickup changes to <mark>3:15</mark> after the break."],
  ["pdf", "Recipe, grandma's rolls.pdf", "Let rise <mark>twice, an hour each</mark>."],
  ["xls", "Mileage log.xlsx"],
  ["chat", "gemini", "Plant care", ["Why are the fiddle leaf leaves brown?", "Usually <mark>too much water</mark>"]],
  ["txt", "Gift ideas.md", "Mom: <mark>the blue scarf</mark> from the market."],
];

function bitHtml(b) {
  const [kind] = b;
  if (kind === "chat") {
    const [, app, title, [me, them]] = b;
    return `<div class="hd"><img src="${MARK[app]}" alt="">${title}</div><p class="say me">${me}</p><p class="say">${them}</p>`;
  }
  if (kind === "pic") return `<div class="pic ${b[1]}"></div><div class="hd">${b[2]}</div>`;
  if (kind === "xls") return `<div class="hd"><span class="tag xls">XLS</span>${b[1]}</div><div class="grid">${"<i></i>".repeat(16)}</div>`;
  const tag = kind.toUpperCase();
  return `<div class="hd"><span class="tag ${kind}">${tag}</span>${b[1]}</div><p>${b[2]}</p>${lines("m", "s")}`;
}

const drift = document.getElementById("drift");
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
let built = 0;

function build() {
  const n = Math.min(14, Math.ceil((innerWidth * 1.2) / 222) + 1);
  if (n === built) return;
  built = n;
  drift.textContent = "";
  for (let c = 0; c < n; c++) {
    const col = document.createElement("div");
    col.className = "col" + (c % 2 ? " down" : "");
    // nearer columns are bigger, brighter and faster
    const near = Math.random();
    col.style.setProperty("--depth", (0.35 + near * 0.45).toFixed(2));
    col.style.setProperty("--size", (0.86 + near * 0.22).toFixed(2));
    col.style.setProperty("--dur", `${Math.round(95 - near * 45)}s`);
    col.style.setProperty("--delay", `${-Math.round(Math.random() * 120)}s`);
    const start = (c * 5) % BITS.length;
    const set = Array.from({ length: 9 }, (_, i) => BITS[(start + i * 3) % BITS.length]);
    // the list twice, so sliding by half loops without a seam
    const html = [...set, ...set].map((b) => `<div class="bit">${bitHtml(b)}</div>`).join("");
    col.innerHTML = html;
    drift.appendChild(col);
  }
}

build();
let t;
addEventListener("resize", () => { clearTimeout(t); t = setTimeout(build, 200); });

if (!still) {
  setInterval(() => {
    if (document.hidden) return;
    const w = innerWidth, h = innerHeight;
    const seen = [...drift.querySelectorAll(".bit:not(.found)")].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.top > 40 && r.bottom < h - 40 && r.left > w * 0.5 && r.right < w - 10 && el.querySelector("mark");
    });
    const el = seen[Math.floor(Math.random() * seen.length)];
    if (!el) return;
    el.classList.add("found");
    setTimeout(() => el.classList.remove("found"), 2600);
  }, 1700);
}
})();
