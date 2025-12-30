import { invoke } from "@tauri-apps/api/core";
import Chart from "chart.js/auto";
import "./style.css";

// Types
interface SystemStatus {
  cpu_usage: number;
  memory_usage: number;
  total_memory: number;
  uptime: number;
  gpu_usage?: number;
  gpu_memory_usage?: number;
  gpu_total_memory?: number;
}

interface ServiceInfo {
  id: number;
  description: string;
  running: boolean;
  pid?: number;
}

// Extend Window interface for global functions
declare global {
  interface Window {
    toggleChart: () => void;
    authenticate: () => Promise<void>;
    refreshAll: () => Promise<void>;
    controlService: (id: number, action: string) => Promise<void>;
    controlAll: (action: string) => Promise<void>;
    quitApp: () => Promise<void>;
  }
}

// State
let passphrase: string = "";
let refreshTimer: number | null = null;
let chart: Chart | null = null;

const chartData = {
  labels: [] as string[],
  datasets: [
    {
      label: "CPU",
      data: [] as number[],
      borderColor: "#ff0000ff",
      tension: 0.4,
      borderWidth: 2,
      pointRadius: 0,
    },
    {
      label: "Memory",
      data: [] as number[],
      borderColor: "#001affff",
      tension: 0.4,
      borderWidth: 2,
      pointRadius: 0,
    },
  ],
};

// Initialization
document.addEventListener("DOMContentLoaded", () => {
  initChart();
  startAutoRefresh();

  // Load passphrase from local storage if available
  const savedPassphrase = localStorage.getItem("remote_helper_passphrase");
  if (savedPassphrase) {
    const tokenInput = document.getElementById(
      "auth-token"
    ) as HTMLInputElement;
    if (tokenInput) {
      tokenInput.value = savedPassphrase;
      window.authenticate();
    }
  }
});

// Chart Setup
function initChart() {
  const canvas = document.getElementById("perfChart") as HTMLCanvasElement;
  if (!canvas) return;

  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  chart = new Chart(ctx, {
    type: "line",
    data: chartData,
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: { legend: { display: false } },
      scales: {
        x: { display: false },
        y: {
          beginAtZero: true,
          max: 100,
          grid: { color: "#1e293b" },
          ticks: { color: "#64748b" },
        },
      },
      animation: false,
    },
  });
}

window.toggleChart = function () {
  const container = document.getElementById("chart-container");
  const chevron = document.getElementById("chart-chevron");
  if (!container || !chevron) return;

  if (container.style.display === "none") {
    container.style.display = "block";
    chevron.style.transform = "rotate(0deg)";
  } else {
    container.style.display = "none";
    chevron.style.transform = "rotate(-90deg)";
  }
};

// Authentication
window.authenticate = async function () {
  const tokenInput = document.getElementById("auth-token") as HTMLInputElement;
  const indicator = document.getElementById("auth-status-indicator");
  if (!tokenInput || !indicator) return;

  passphrase = tokenInput.value;

  try {
    await invoke("authenticate", { passphrase: passphrase });
    // Success
    indicator.innerHTML = `<svg class="w-5 h-5 text-emerald-500" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path></svg>`;
    localStorage.setItem("remote_helper_passphrase", passphrase);
    window.refreshAll();
  } catch (e) {
    // Error
    indicator.innerHTML = `<svg class="w-5 h-5 text-rose-500" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path></svg>`;
    console.error("Auth failed:", e);
  }
};

// Data Fetching
window.refreshAll = async function () {
  if (!passphrase) return;
  await Promise.all([getStatus(), listServices()]);
};

async function getStatus() {
  try {
    const status = await invoke<SystemStatus>("get_status");
    updateStatusUI(status);
    updateChart(status);
  } catch (e) {
    console.error("Get status failed:", e);
  }
}

async function listServices() {
  try {
    const services = await invoke<ServiceInfo[]>("list_services");
    renderServices(services);
  } catch (e) {
    console.error("List services failed:", e);
  }
}

// UI Updates
function updateStatusUI(status: SystemStatus) {
  const cpuVal = document.getElementById("cpu-val");
  const cpuBar = document.getElementById("cpu-bar");
  const memVal = document.getElementById("mem-val");
  const memBar = document.getElementById("mem-bar");
  const uptimeVal = document.getElementById("uptime-val");

  if (cpuVal) cpuVal.textContent = status.cpu_usage.toFixed(1) + "%";
  if (cpuBar) cpuBar.style.width = status.cpu_usage + "%";

  if (memVal && memBar) {
    const usedGB = (status.memory_usage / 1024 / 1024 / 1024).toFixed(1);
    const totalGB = (status.total_memory / 1024 / 1024 / 1024).toFixed(1);
    const memPercent = (status.memory_usage / status.total_memory) * 100;
    memVal.textContent = `${usedGB} GB / ${totalGB} GB`;
    memBar.style.width = memPercent + "%";
  }

  if (uptimeVal) {
    const hours = Math.floor(status.uptime / 3600);
    const minutes = Math.floor((status.uptime % 3600) / 60);
    const seconds = status.uptime % 60;
    uptimeVal.textContent = `${hours.toString().padStart(2, "0")}:${minutes
      .toString()
      .padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`;
  }

  const gpuVal = document.getElementById("gpu-val");
  const gpuBar = document.getElementById("gpu-bar");

  if (gpuVal && gpuBar) {
    if (status.gpu_usage !== undefined && status.gpu_usage !== null) {
      let text = `${status.gpu_usage}%`;
      if (
        status.gpu_memory_usage !== undefined &&
        status.gpu_total_memory !== undefined
      ) {
        const usedGB = (status.gpu_memory_usage / 1024 / 1024 / 1024).toFixed(
          1
        );
        const totalGB = (status.gpu_total_memory / 1024 / 1024 / 1024).toFixed(
          1
        );
        text += ` (${usedGB}G / ${totalGB}G)`;
      }
      gpuVal.textContent = text;
      gpuBar.style.width = `${status.gpu_usage}%`;
    } else {
      gpuVal.textContent = "N/A";
      gpuBar.style.width = "0%";
    }
  }
}

function updateChart(status: SystemStatus) {
  if (!chart) return;

  const now = new Date().toLocaleTimeString();
  if (chartData.labels.length > 20) {
    chartData.labels.shift();
    chartData.datasets[0].data.shift();
    chartData.datasets[1].data.shift();
  }
  chartData.labels.push(now);
  chartData.datasets[0].data.push(status.cpu_usage);
  chartData.datasets[1].data.push(
    (status.memory_usage / status.total_memory) * 100
  );
  chart.update();
}

function renderServices(services: ServiceInfo[]) {
  const tbody = document.getElementById("service-list-body");
  if (!tbody) return;

  // Diff logic to prevent full re-render flicker
  const existingRows = Array.from(tbody.children) as HTMLTableRowElement[];
  const newIds = new Set(services.map(s => s.id));

  // Remove old rows
  existingRows.forEach(row => {
    const id = parseInt(row.dataset.id || "0");
    if (id && !newIds.has(id)) {
      row.remove();
    }
  });
  
  // Clear placeholder if services exist
  if (services.length > 0 && tbody.querySelector('td[colspan="4"]')) {
      tbody.innerHTML = '';
  }

  services.forEach((svc, index) => {
    let tr = tbody.querySelector(`tr[data-id="${svc.id}"]`) as HTMLTableRowElement;
    
    if (!tr) {
      tr = document.createElement("tr");
      tr.dataset.id = svc.id.toString();
      tr.className = "hover:bg-slate-800/50 transition-colors animate-slide-up opacity-0 border-b border-slate-800/50 last:border-0";
      tr.style.animationDelay = `${index * 50}ms`;
      tbody.appendChild(tr);
    }

    const statusBadge = svc.running
      ? `<span class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 shadow-[0_0_10px_rgba(16,185,129,0.2)]">
                <span class="w-1.5 h-1.5 mr-1.5 bg-emerald-400 rounded-full animate-pulse"></span>运行中 (PID: ${svc.pid})
               </span>`
      : `<span class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-rose-500/10 text-rose-400 border border-rose-500/20">
                已停止
               </span>`;

    tr.innerHTML = `
            <td class="px-6 py-4 font-mono text-slate-500 text-xs">#${svc.id}</td>
            <td class="px-6 py-4 font-medium text-slate-200">${svc.description}</td>
            <td class="px-6 py-4">${statusBadge}</td>
            <td class="px-6 py-4 text-right space-x-2">
                <button onclick="controlService(${svc.id}, 'start')" 
                    class="text-xs px-3 py-1.5 rounded border border-slate-700 hover:bg-emerald-900/30 hover:text-emerald-400 hover:border-emerald-800 transition-all duration-200 disabled:opacity-30 disabled:cursor-not-allowed active:scale-95"
                    ${svc.running ? "disabled" : ""}>启动</button>
                <button onclick="controlService(${svc.id}, 'stop')" 
                    class="text-xs px-3 py-1.5 rounded border border-slate-700 hover:bg-rose-900/30 hover:text-rose-400 hover:border-rose-800 transition-all duration-200 disabled:opacity-30 disabled:cursor-not-allowed active:scale-95"
                    ${!svc.running ? "disabled" : ""}>停止</button>
                <button onclick="controlService(${svc.id}, 'restart')" 
                    class="text-xs px-3 py-1.5 rounded border border-slate-700 hover:bg-indigo-900/30 hover:text-indigo-400 hover:border-indigo-800 transition-all duration-200 disabled:opacity-30 disabled:cursor-not-allowed active:scale-95"
                    ${!svc.running ? "disabled" : ""}>重启</button>
            </td>
        `;
  });
  
  if (services.length === 0 && tbody.children.length === 0) {
     tbody.innerHTML = `<tr><td colspan="4" class="px-6 py-8 text-center text-slate-500 animate-pulse-soft">暂无服务或未连接</td></tr>`;
  }
}

// Actions
window.controlService = async function (id: number, action: string) {
  try {
    await invoke("control_service", { id, action });
    setTimeout(listServices, 500); // Quick refresh
  } catch (e) {
    alert(`操作失败: ${e}`);
  }
};

window.controlAll = async function (action: string) {
  // This is a bit naive, ideally backend should support batch operations
  // But for now we iterate over the current list
  try {
    const services = await invoke<ServiceInfo[]>("list_services");
    for (const svc of services) {
      if (action === "start" && !svc.running) {
        window.controlService(svc.id, "start");
      } else if (action === "stop" && svc.running) {
        window.controlService(svc.id, "stop");
      }
    }
  } catch (e) {
    console.error("Control all failed:", e);
  }
};

window.quitApp = async function () {
  try {
    await invoke("quit_app");
  } catch (e) {
    console.error("Quit app failed:", e);
  }
};

// Timer Logic
function startAutoRefresh() {
  const select = document.getElementById(
    "refresh-interval"
  ) as HTMLSelectElement;
  const display = document.getElementById("refresh-display");
  if (!select) return;

  const updateTimer = () => {
    if (refreshTimer) clearInterval(refreshTimer);
    const interval = parseInt(select.value);
    if (interval > 0) {
      refreshTimer = window.setInterval(window.refreshAll, interval);
    }
    
    // Update display text
    if (display) {
      const selectedOption = select.options[select.selectedIndex];
      display.textContent = selectedOption.text;
    }
  };

  select.addEventListener("change", updateTimer);
  updateTimer(); // Start immediately
}
