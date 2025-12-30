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
  cpu_model: string;
  gpu_model?: string;
}

interface ServiceInfo {
  id: number;
  description: string;
  running: boolean;
  pid?: number;
}

interface CustomServiceConfig {
  description: string;
  exe_path: string;
  args: string;
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
    openAddServiceModal: () => void;
    closeAddServiceModal: () => void;
    confirmAddService: () => void;
    registerAndStart: (index: number) => Promise<void>;
    deleteCustomService: (index: number) => void;
  }
}

// State
let passphrase: string = "";
let refreshTimer: number | null = null;
let chart: Chart | null = null;
let isRefreshing: boolean = false;
let customServices: CustomServiceConfig[] = [];

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
    {
      label: "GPU",
      data: [] as number[],
      borderColor: "#a855f7", // Purple-500
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

  // Load address
  const savedAddress = localStorage.getItem("remote_helper_address");
  if (savedAddress) {
      const addressInput = document.getElementById("server-address") as HTMLInputElement;
      if (addressInput) {
          addressInput.value = savedAddress;
      }
  }

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

  // Load custom services
  const savedServices = localStorage.getItem("remote_helper_custom_services");
  if (savedServices) {
      try {
          customServices = JSON.parse(savedServices);
      } catch (e) {
          console.error("Failed to load custom services", e);
      }
  }
});

// Chart Setup
function initChart() {
  const canvas = document.getElementById("perfChart") as HTMLCanvasElement;
  if (!canvas) return;

  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  // Custom plugin to draw labels at the end of lines
  const endLabelPlugin = {
    id: 'endLabel',
    afterDatasetsDraw(chart: any) {
      const { ctx } = chart;
      chart.data.datasets.forEach((dataset: any, i: number) => {
        const meta = chart.getDatasetMeta(i);
        if (!meta.hidden && meta.data.length > 0) {
          const lastPoint = meta.data[meta.data.length - 1];
          ctx.save();
          ctx.font = 'bold 12px sans-serif';
          ctx.fillStyle = dataset.borderColor;
          ctx.textAlign = 'left';
          ctx.textBaseline = 'middle';
          ctx.fillText(dataset.label, lastPoint.x + 8, lastPoint.y);
          ctx.restore();
        }
      });
    }
  };

  chart = new Chart(ctx, {
    type: "line",
    data: chartData,
    options: {
      responsive: true,
      maintainAspectRatio: false,
      layout: {
        padding: {
          right: 60 // Space for labels
        }
      },
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
    plugins: [endLabelPlugin]
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
  const addressInput = document.getElementById("server-address") as HTMLInputElement;
  const indicator = document.getElementById("auth-status-indicator");
  if (!tokenInput || !indicator || !addressInput) return;

  passphrase = tokenInput.value;
  const address = addressInput.value;

  try {
    await invoke("authenticate", { passphrase: passphrase, address: address });
    // Success
    indicator.innerHTML = `<svg class="w-5 h-5 text-emerald-500" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path></svg>`;
    localStorage.setItem("remote_helper_passphrase", passphrase);
    localStorage.setItem("remote_helper_address", address);
    window.refreshAll();
  } catch (e) {
    // Error
    indicator.innerHTML = `<svg class="w-5 h-5 text-rose-500" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path></svg>`;
    console.error("Auth failed:", e);
  }
};

// Data Fetching
window.refreshAll = async function () {
  if (isRefreshing) return;
  if (!passphrase) return;
  
  isRefreshing = true;
  try {
    // Get current interval to send to server
    const select = document.getElementById("refresh-interval") as HTMLSelectElement;
    let interval = 3000;
    if (select) {
        const val = select.value;
        if (val === "custom") {
            // For custom, we need to store the actual value somewhere, 
            // but for now let's assume the timer logic handles the polling frequency
            // and we just want to tell the server a reasonable update rate.
            // We can read it from the timer or a stored variable.
            // Let's use the global refreshTimer interval if possible, or default to 1000.
            // Actually, let's store the custom value in a data attribute or variable.
            const customVal = parseInt(select.dataset.customValue || "1000");
            interval = customVal;
        } else {
            interval = parseInt(val);
        }
    }
    
    // Ensure interval is valid for server (>= 100ms)
    if (interval < 100 && interval !== 0) interval = 100;
    if (interval === 0) interval = 3000; // If paused, tell server to keep default or last

    await Promise.all([getStatus(interval), listServices()]);
  } finally {
    isRefreshing = false;
  }
};

async function getStatus(interval: number) {
  try {
    const status = await invoke<SystemStatus>("get_status", { intervalMs: interval });
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
  const cpuModel = document.getElementById("cpu-model");
  const memVal = document.getElementById("mem-val");
  const memBar = document.getElementById("mem-bar");
  const uptimeVal = document.getElementById("uptime-val");

  if (cpuVal) cpuVal.textContent = status.cpu_usage.toFixed(1) + "%";
  if (cpuBar) cpuBar.style.width = status.cpu_usage + "%";
  if (cpuModel) cpuModel.textContent = status.cpu_model;

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
  const gpuModel = document.getElementById("gpu-model");

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
      if (gpuModel && status.gpu_model) gpuModel.textContent = status.gpu_model;
    } else {
      gpuVal.textContent = "N/A";
      gpuBar.style.width = "0%";
      if (gpuModel) gpuModel.textContent = "";
    }
  }
}

function updateChart(status: SystemStatus) {
  if (!chart) return;

  const now = new Date().toLocaleTimeString();
  if (chartData.labels.length > 3000) {
    chartData.labels.shift();
    chartData.datasets[0].data.shift();
    chartData.datasets[1].data.shift();
    chartData.datasets[2].data.shift();
  }
  chartData.labels.push(now);
  chartData.datasets[0].data.push(status.cpu_usage);
  chartData.datasets[1].data.push(
    (status.memory_usage / status.total_memory) * 100
  );
  chartData.datasets[2].data.push(status.gpu_usage ?? 0);
  chart.update();
}

function renderServices(services: ServiceInfo[]) {
  const tbody = document.getElementById("service-list-body");
  if (!tbody) return;

  // Merge server services and local custom services
  const displayItems: { type: 'server' | 'local', data: ServiceInfo, index?: number }[] = [];
  const coveredDescriptions = new Set<string>();

  services.forEach(s => {
      displayItems.push({ type: 'server', data: s });
      coveredDescriptions.add(s.description);
  });

  customServices.forEach((cs, index) => {
      if (!coveredDescriptions.has(cs.description)) {
          displayItems.push({
              type: 'local',
              data: {
                  id: -1,
                  description: cs.description,
                  running: false,
                  pid: undefined
              },
              index: index
          });
      }
  });

  // Diff logic to prevent full re-render flicker
  const existingRows = Array.from(tbody.children) as HTMLTableRowElement[];
  // Create a set of keys for current items. Server: "s-{id}", Local: "l-{index}"
  const newKeys = new Set(displayItems.map(item => item.type === 'server' ? `s-${item.data.id}` : `l-${item.index}`));

  // Remove old rows
  existingRows.forEach(row => {
    const key = row.dataset.key;
    // If row has no key (legacy) or key is not in new set, remove it
    if (!key || !newKeys.has(key)) {
      row.remove();
    }
  });
  
  // Clear placeholder if services exist
  if (displayItems.length > 0 && tbody.querySelector('td[colspan="4"]')) {
      tbody.innerHTML = '';
  }

  displayItems.forEach((item, i) => {
    const key = item.type === 'server' ? `s-${item.data.id}` : `l-${item.index}`;
    let tr = tbody.querySelector(`tr[data-key="${key}"]`) as HTMLTableRowElement;
    
    if (!tr) {
      tr = document.createElement("tr");
      tr.dataset.key = key;
      tr.className = "hover:bg-slate-800/50 transition-colors animate-slide-up opacity-0 border-b border-slate-800/50 last:border-0";
      tr.style.animationDelay = `${i * 50}ms`;
      tbody.appendChild(tr);
    }

    const svc = item.data;
    const statusBadge = svc.running
      ? `<span class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 shadow-[0_0_10px_rgba(16,185,129,0.2)]">
                <span class="w-1.5 h-1.5 mr-1.5 bg-emerald-400 rounded-full animate-pulse"></span>运行中 (PID: ${svc.pid})
               </span>`
      : `<span class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-rose-500/10 text-rose-400 border border-rose-500/20">
                ${item.type === 'local' ? '未注册' : '已停止'}
               </span>`;

    let actions = '';
    if (item.type === 'server') {
        actions = `
            <button onclick="controlService(${svc.id}, 'start')" 
                class="text-xs px-3 py-1.5 rounded border border-slate-700 hover:bg-emerald-900/30 hover:text-emerald-400 hover:border-emerald-800 transition-all duration-200 disabled:opacity-30 disabled:cursor-not-allowed active:scale-95"
                ${svc.running ? "disabled" : ""}>启动</button>
            <button onclick="controlService(${svc.id}, 'stop')" 
                class="text-xs px-3 py-1.5 rounded border border-slate-700 hover:bg-rose-900/30 hover:text-rose-400 hover:border-rose-800 transition-all duration-200 disabled:opacity-30 disabled:cursor-not-allowed active:scale-95"
                ${!svc.running ? "disabled" : ""}>停止</button>
            <button onclick="controlService(${svc.id}, 'restart')" 
                class="text-xs px-3 py-1.5 rounded border border-slate-700 hover:bg-indigo-900/30 hover:text-indigo-400 hover:border-indigo-800 transition-all duration-200 disabled:opacity-30 disabled:cursor-not-allowed active:scale-95"
                ${!svc.running ? "disabled" : ""}>重启</button>
        `;
    } else {
        actions = `
            <button onclick="registerAndStart(${item.index})" 
                class="text-xs px-3 py-1.5 rounded border border-slate-700 hover:bg-emerald-900/30 hover:text-emerald-400 hover:border-emerald-800 transition-all duration-200 active:scale-95">
                启动
            </button>
            <button onclick="deleteCustomService(${item.index})" 
                class="text-xs px-3 py-1.5 rounded border border-slate-700 hover:bg-rose-900/30 hover:text-rose-400 hover:border-rose-800 transition-all duration-200 active:scale-95">
                删除
            </button>
        `;
    }

    tr.innerHTML = `
            <td class="px-6 py-4 font-mono text-slate-500 text-xs">${item.type === 'server' ? '#' + svc.id : 'Local'}</td>
            <td class="px-6 py-4 font-medium text-slate-200">${svc.description}</td>
            <td class="px-6 py-4">${statusBadge}</td>
            <td class="px-6 py-4 text-right space-x-2">
                ${actions}
            </td>
        `;
  });
  
  if (displayItems.length === 0 && tbody.children.length === 0) {
     tbody.innerHTML = `<tr><td colspan="4" class="px-6 py-8 text-center text-slate-500 animate-pulse-soft">暂无服务或未连接</td></tr>`;
  }
}

window.openAddServiceModal = function() {
    document.getElementById("add-service-modal")?.classList.remove("hidden");
}
window.closeAddServiceModal = function() {
    document.getElementById("add-service-modal")?.classList.add("hidden");
}
window.confirmAddService = function() {
    const desc = (document.getElementById("new-svc-desc") as HTMLInputElement).value;
    const path = (document.getElementById("new-svc-path") as HTMLInputElement).value;
    const args = (document.getElementById("new-svc-args") as HTMLInputElement).value;
    
    if (!desc || !path) {
        alert("请填写描述和路径");
        return;
    }
    
    customServices.push({ description: desc, exe_path: path, args: args });
    localStorage.setItem("remote_helper_custom_services", JSON.stringify(customServices));
    window.closeAddServiceModal();
    window.refreshAll();
}

window.registerAndStart = async function(index: number) {
    const svc = customServices[index];
    if (!svc) return;
    
    try {
        const argsVec = svc.args.split(" ").filter(s => s.length > 0);
        const id = await invoke<number>("add_service", { 
            description: svc.description, 
            exePath: svc.exe_path, 
            args: argsVec 
        });
        await window.controlService(id, "start");
    } catch (e) {
        alert("启动失败: " + e);
    }
}

window.deleteCustomService = function(index: number) {
    if (confirm("确定要删除此服务配置吗？")) {
        customServices.splice(index, 1);
        localStorage.setItem("remote_helper_custom_services", JSON.stringify(customServices));
        window.refreshAll();
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
        await window.controlService(svc.id, "start");
      } else if (action === "stop" && svc.running) {
        await window.controlService(svc.id, "stop");
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
    
    let interval = 0;
    if (select.value === "custom") {
        const input = prompt("请输入刷新间隔 (毫秒, 最低100):", "1000");
        if (input) {
            let val = parseInt(input);
            if (isNaN(val) || val < 100) {
                alert("无效的间隔，已重置为 100ms");
                val = 100;
            }
            interval = val;
            select.dataset.customValue = val.toString();
            if (display) display.textContent = `${val}ms`;
        } else {
            // Cancelled, revert to default
            select.value = "3000";
            interval = 3000;
            if (display) display.textContent = "3s";
        }
    } else {
        interval = parseInt(select.value);
        // Update display text
        if (display) {
            const selectedOption = select.options[select.selectedIndex];
            display.textContent = selectedOption.text;
        }
    }

    if (interval > 0) {
      refreshTimer = window.setInterval(window.refreshAll, interval);
      // Trigger immediate refresh when changing interval
      window.refreshAll();
    }
  };

  select.addEventListener("change", updateTimer);
  updateTimer(); // Start immediately
}
