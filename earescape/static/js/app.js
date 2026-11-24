// === 1. 취향 진단 프리셋 ===
function applySoundPreset(bass, mid, treble) {
  const b = document.getElementById("slider-bass");
  const m = document.getElementById("slider-mid");
  const t = document.getElementById("slider-treble");
  if (!b || !m || !t) return;
  b.value = bass;
  m.value = mid;
  t.value = treble;
  document.getElementById("val-bass").textContent = bass + " / 5";
  document.getElementById("val-mid").textContent = mid + " / 5";
  document.getElementById("val-treble").textContent = treble + " / 5";
}

function setBudget(manwon) {
  const el = document.getElementById("slider-budget");
  if (!el) return;
  el.value = manwon;
  document.getElementById("val-budget").textContent = manwon + "만 원 이하";
}

function fillChatPrompt(text) {
  const input = document.getElementById("chat-query-input");
  if (input) {
    input.value = text;
    input.focus();
  }
}

// === 2. 비교함 (localStorage 최대 4개) ===
const COMPARE_KEY = "earescape_compare_items";

function getCompareItems() {
  try {
    return JSON.parse(localStorage.getItem(COMPARE_KEY) || "[]");
  } catch {
    return [];
  }
}

function saveCompareItems(items) {
  localStorage.setItem(COMPARE_KEY, JSON.stringify(items.slice(0, 4)));
  syncCompareTrayUI();
}

function toggleCompareItem(id, name) {
  let items = getCompareItems();
  const exists = items.find((i) => i.id === id);
  if (exists) {
    items = items.filter((i) => i.id !== id);
  } else {
    if (items.length >= 4) {
      alert("비교함은 최대 4개 제품까지 담을 수 있습니다.");
      return;
    }
    items.push({ id, name });
  }
  saveCompareItems(items);
}

function clearCompare() {
  saveCompareItems([]);
}

function goToCompare() {
  const items = getCompareItems();
  if (items.length === 0) return;
  const ids = items.map((i) => encodeURIComponent(i.id)).join(",");
  window.location.href = "/compare?ids=" + ids;
}

function syncCompareTrayUI() {
  const items = getCompareItems();
  const tray = document.getElementById("compare-tray");
  const navBadge = document.getElementById("nav-compare-count");
  if (navBadge) {
    navBadge.textContent = String(items.length);
    navBadge.classList.toggle("hidden", items.length === 0);
  }
  if (!tray) return;
  if (items.length === 0) {
    tray.classList.add("hidden");
    tray.classList.remove("flex");
  } else {
    tray.classList.remove("hidden");
    tray.classList.add("flex");
    document.getElementById("tray-count").textContent = String(items.length);
    document.getElementById("tray-names").textContent = items.map((i) => i.name).join(" · ");
  }
}

// 독립 검증 수정: ?ids=가 없는 기본 3종 상태에서도 compare-data JSON에서 현재 ID 목록을 읽어 정상 추가/제거
function getActiveCompareIdsFromPage() {
  const url = new URL(window.location.href);
  const fromQuery = (url.searchParams.get("ids") || "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  if (fromQuery.length > 0) return fromQuery;

  const dataEl = document.getElementById("compare-data");
  if (dataEl) {
    try {
      const products = JSON.parse(dataEl.textContent || "[]");
      return products.map((p) => p.id).filter(Boolean);
    } catch {
      return [];
    }
  }
  return [];
}

function addSelectedProductToUrl() {
  const sel = document.getElementById("add-compare-select");
  if (!sel || !sel.value) return;
  const url = new URL(window.location.href);
  const current = getActiveCompareIdsFromPage();
  if (!current.includes(sel.value)) {
    if (current.length >= 4) {
      alert("최대 4개 제품까지 비교할 수 있습니다.");
      return;
    }
    current.push(sel.value);
  }
  url.searchParams.set("ids", current.join(","));
  window.location.href = url.toString();
}

function removeProductFromUrl(id) {
  const url = new URL(window.location.href);
  const current = getActiveCompareIdsFromPage().filter((s) => s !== id);
  if (current.length > 0) {
    url.searchParams.set("ids", current.join(","));
  } else {
    url.searchParams.delete("ids");
  }
  window.location.href = url.toString();
}

function toggleDiffOnlyRows() {
  const checked = document.getElementById("diff-only-toggle")?.checked;
  const rows = document.querySelectorAll("#compare-spec-table tbody tr.spec-row");
  rows.forEach((row) => {
    const cells = Array.from(row.querySelectorAll("td")).slice(1);
    const values = new Set(cells.map((td) => td.textContent.trim()));
    if (checked && values.size <= 1) {
      row.classList.add("hidden");
    } else {
      row.classList.remove("hidden");
    }
  });
}

// === 3. 6축 사운드 레이더 차트 (순수 Canvas) ===
function renderRadarChartFromPage() {
  const dataEl = document.getElementById("compare-data");
  const canvas = document.getElementById("sound-radar-canvas");
  if (!dataEl || !canvas) return;
  let products = [];
  try {
    products = JSON.parse(dataEl.textContent || "[]");
  } catch {
    return;
  }
  if (products.length === 0) return;

  const ctx = canvas.getContext("2d");
  const w = canvas.width;
  const h = canvas.height;
  const cx = w / 2;
  const cy = h / 2 + 6;
  const radius = Math.min(w, h) * 0.34;
  const axes = [
    { key: "bass", label: "저음" },
    { key: "mid", label: "중음·보컬" },
    { key: "treble", label: "고음" },
    { key: "soundstage", label: "공간감" },
    { key: "isolation_anc", label: "ANC·차음" },
    { key: "comfort", label: "착용감" },
  ];
  const palette = ["#C85A32", "#1F5C6E", "#5B4B8A", "#2E7D5B"];

  ctx.clearRect(0, 0, w, h);

  for (let level = 2; level <= 10; level += 2) {
    const r = (radius * level) / 10;
    ctx.beginPath();
    axes.forEach((_, idx) => {
      const angle = (Math.PI * 2 * idx) / axes.length - Math.PI / 2;
      const x = cx + r * Math.cos(angle);
      const y = cy + r * Math.sin(angle);
      if (idx === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.closePath();
    ctx.strokeStyle = "#E4E1DA";
    ctx.lineWidth = 1;
    ctx.stroke();
  }

  ctx.font = '600 12px "Pretendard Variable", sans-serif';
  ctx.fillStyle = "#63615B";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  axes.forEach((ax, idx) => {
    const angle = (Math.PI * 2 * idx) / axes.length - Math.PI / 2;
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.lineTo(cx + radius * Math.cos(angle), cy + radius * Math.sin(angle));
    ctx.strokeStyle = "#E4E1DA";
    ctx.stroke();

    const lx = cx + (radius + 24) * Math.cos(angle);
    const ly = cy + (radius + 20) * Math.sin(angle);
    ctx.fillText(ax.label, lx, ly);
  });

  const legendEl = document.getElementById("radar-legend");
  if (legendEl) legendEl.innerHTML = "";

  products.slice(0, 4).forEach((p, pIdx) => {
    const color = palette[pIdx % palette.length];
    const sp = p.sound_profile;
    ctx.beginPath();
    axes.forEach((ax, idx) => {
      const val = Number(sp[ax.key] || 5);
      const r = (radius * val) / 10;
      const angle = (Math.PI * 2 * idx) / axes.length - Math.PI / 2;
      const x = cx + r * Math.cos(angle);
      const y = cy + r * Math.sin(angle);
      if (idx === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.closePath();
    ctx.fillStyle = color + "24";
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.fill();
    ctx.stroke();

    if (legendEl) {
      const chip = document.createElement("span");
      chip.className = "inline-flex items-center gap-1.5 rounded-full border border-[#E4E1DA] bg-[#FAF9F6] px-2.5 py-1";
      chip.innerHTML = `<span style="background:${color}" class="inline-block h-2.5 w-2.5 rounded-full"></span><span>${p.brand} ${p.name}</span>`;
      legendEl.appendChild(chip);
    }
  });
}

// === 4. 온디맨드 [데이터 수집] 모달 및 POST SSE 스트림 ===
function openCollectModal() {
  const m = document.getElementById("collect-modal");
  if (m) {
    m.classList.remove("hidden");
    m.classList.add("flex");
  }
}

function closeCollectModal() {
  const m = document.getElementById("collect-modal");
  if (m) {
    m.classList.add("hidden");
    m.classList.remove("flex");
  }
}

async function startCollectionStream() {
  const btn = document.getElementById("btn-start-collect");
  const statusEl = document.getElementById("collect-status-text");
  const pctEl = document.getElementById("collect-progress-pct");
  const barEl = document.getElementById("collect-progress-bar");
  const logEl = document.getElementById("collect-stream-log");

  if (btn) btn.disabled = true;
  logEl.innerHTML = "";
  barEl.style.width = "5%";
  pctEl.textContent = "5%";
  statusEl.textContent = "TOP 10 브랜드 데이터 수집을 시작합니다...";

  try {
    const resp = await fetch("/api/collect/run", { method: "POST" });
    const reader = resp.body.getReader();
    const decoder = new TextDecoder("utf-8");
    let buffer = "";

    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const parts = buffer.split("\n\n");
      buffer = parts.pop() || "";

      for (const chunk of parts) {
        const line = chunk.split("\n").find((l) => l.startsWith("data:"));
        if (!line) continue;
        const jsonStr = line.slice(5).trim();
        if (!jsonStr) continue;
        const ev = JSON.parse(jsonStr);

        if (ev.stage === "started") {
          appendLog(logEl, `[시작] 실행 ID ${ev.run_id} · 검증 대상 ${ev.total}개 제품`);
        } else if (ev.stage === "checking") {
          statusEl.textContent = `수집 중 (${ev.index}/${ev.total}): ${ev.brand} ${ev.product_name}`;
        } else if (ev.stage === "item_done") {
          const pct = Math.round((ev.index / Math.max(ev.total, 1)) * 100);
          barEl.style.width = pct + "%";
          pctEl.textContent = pct + "%";
          const tag = ev.status === "live_ok" ? "LIVE" : "SEED";
          appendLog(logEl, `[${tag}] ${ev.brand} ${ev.product_name} (${ev.policy_label}) — ${ev.message}`);
        } else if (ev.stage === "completed") {
          barEl.style.width = "100%";
          pctEl.textContent = "100%";
          statusEl.textContent = "수집 및 RAG 재색인 완료";
          appendLog(logEl, `[완료] ${ev.summary_message}`);
          if (window.htmx) {
            htmx.ajax("GET", "/api/collect/logs", "#collect-history-box");
          }
          const footerTs = document.getElementById("footer-last-collected");
          if (footerTs) footerTs.textContent = "방금 전 갱신됨";
        } else if (ev.stage === "failed") {
          statusEl.textContent = "수집 실패";
          appendLog(logEl, `[오류] ${ev.message}`);
        }
      }
    }
  } catch (err) {
    statusEl.textContent = "수집 연결 오류";
    appendLog(logEl, `[오류] ${err}`);
  } finally {
    if (btn) btn.disabled = false;
  }
}

function appendLog(container, text) {
  const p = document.createElement("p");
  p.textContent = text;
  container.appendChild(p);
  container.scrollTop = container.scrollHeight;
}

// === 5. 음향 사전 드로어 & /chat ?q= 자동 채우기 ===
function openGlossaryDrawer() {
  const d = document.getElementById("glossary-drawer");
  if (d) {
    d.classList.remove("hidden");
    d.classList.add("flex");
  }
}

function closeGlossaryDrawer() {
  const d = document.getElementById("glossary-drawer");
  if (d) {
    d.classList.add("hidden");
    d.classList.remove("flex");
  }
}

document.addEventListener("DOMContentLoaded", function () {
  syncCompareTrayUI();
  const qParam = new URLSearchParams(window.location.search).get("q");
  if (qParam) {
    fillChatPrompt(qParam);
  }
});
