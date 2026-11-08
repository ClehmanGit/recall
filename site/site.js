// Two small touches: the demo search types itself once before its results
// appear, and the page fits the window. No network, no storage.

(() => {
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;

const typed = document.getElementById("typed");
const demo = document.querySelector(".demo");
if (typed && demo && !still && getComputedStyle(demo).display !== "none") {
  const text = typed.textContent;
  typed.textContent = "";
  demo.classList.add("typing");
  let i = 0;
  setTimeout(function step() {
    typed.textContent = text.slice(0, ++i);
    if (i < text.length) setTimeout(step, 45 + Math.random() * 55);
    else setTimeout(() => demo.classList.remove("typing"), 250);
  }, 700);
}

// No scrolling on any laptop or desktop window: if the page is still taller than
// the window after the CSS has done what it can, shrink it just enough to fit.
// Phones scroll normally.
const page = document.querySelector(".page");
function fit() {
  if (!page) return;
  let z = 1;
  page.style.setProperty("--fit", "1");
  if (innerWidth <= 600) return;
  // text reflows a little once scaled, so measure again until it truly fits
  for (let i = 0; i < 4; i++) {
    const need = document.documentElement.scrollHeight;
    if (need <= innerHeight || z <= 0.7) break;
    z = Math.max(0.7, Math.floor(z * (innerHeight / need) * 1000) / 1000 - 0.002);
    page.style.setProperty("--fit", z.toFixed(3));
  }
}
fit();
// and again once the web fonts have swapped in, since they change line heights
document.fonts?.ready.then(fit);
document.fonts?.addEventListener?.("loadingdone", () => fit());
addEventListener("load", () => fit());
// re-check shortly after every resize, and once more in case layout was still settling
// (also when the window size changes without a resize event, as some tools do)
let ft, ft2, seen = `${innerWidth}x${innerHeight}`;
function schedule() {
  clearTimeout(ft); clearTimeout(ft2);
  ft = setTimeout(fit, 120);
  ft2 = setTimeout(fit, 500);
}
addEventListener("resize", schedule);
// and whenever the content itself changes height (a late font swap, for one)
const hero = document.querySelector(".hero");
let heroH = 0;
if ("ResizeObserver" in window && hero) new ResizeObserver(() => {
  const h = Math.round(hero.getBoundingClientRect().height / (parseFloat(page.style.getPropertyValue("--fit")) || 1));
  if (Math.abs(h - heroH) > 1) { heroH = h; schedule(); }
}).observe(hero);
if ("ResizeObserver" in window) new ResizeObserver(() => {
  const now = `${innerWidth}x${innerHeight}`;
  if (now !== seen) { seen = now; schedule(); }
}).observe(document.documentElement);
})();
