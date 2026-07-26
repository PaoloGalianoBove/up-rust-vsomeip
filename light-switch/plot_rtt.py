import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import os

FILE_UPROTOCOL = "log_rtt router.csv"

def load_uprotocol(path):
    rtt_values = []
    proc_ram = []
    proc_vsz = []
    proc_cpu = []
    sys_ram = []
    sys_cpu = []
    
    with open(path, "r") as f:
        # Skip header
        header = f.readline()
        for line in f:
            parts = line.strip().split(",")
            if len(parts) >= 8:
                try:
                    rtt_values.append(float(parts[1]))
                    proc_ram.append(float(parts[3]))
                    proc_vsz.append(float(parts[4]))
                    proc_cpu.append(float(parts[5]))
                    sys_ram.append(float(parts[6]))
                    sys_cpu.append(float(parts[7]))
                except ValueError:
                    pass
    return (
        np.asarray(rtt_values),
        np.asarray(proc_ram),
        np.asarray(proc_vsz),
        np.asarray(proc_cpu),
        np.asarray(sys_ram),
        np.asarray(sys_cpu)
    )

# Load data
rtt, proc_ram, proc_vsz, proc_cpu, sys_ram, sys_cpu = load_uprotocol(FILE_UPROTOCOL)

# Complete RTT statistics (computed on all data, including outliers)
rtt_stats = {
    "N": len(rtt),
    "mean": np.mean(rtt),
    "median": np.median(rtt),
    "p95": np.percentile(rtt, 95),
    "min": np.min(rtt),
    "max": np.max(rtt),
}

# Dynamically define range to center the bell curve with padding on both sides
rtt_min = np.min(rtt)
rtt_99_5 = np.percentile(rtt, 99.5)

X_MIN = max(0.0, rtt_min - 0.05)
X_MAX = rtt_99_5 + 0.15

rtt_plot = rtt[(rtt >= X_MIN) & (rtt <= X_MAX)]

# Get system core count and memory capacity to calculate relative percentages
num_cores = os.cpu_count() or 1

total_mem_mb = 16000.0  # Default fallback
try:
    with open("/proc/meminfo", "r") as f:
        for line in f:
            if line.startswith("MemTotal:"):
                total_mem_mb = float(line.split()[1]) / 1024.0
                break
except Exception:
    pass

# CPU metrics (relative to 1 core, and relative to system total)
cpu_mean_core = np.mean(proc_cpu)
cpu_median_core = np.median(proc_cpu)
cpu_mean_total = cpu_mean_core / num_cores
cpu_median_total = cpu_median_core / num_cores

# RAM metrics (absolute in MB, and relative to system total)
ram_mean_mb = np.mean(proc_ram)
ram_median_mb = np.median(proc_ram)
ram_mean_pct = (ram_mean_mb / total_mem_mb) * 100.0
ram_median_pct = (ram_median_mb / total_mem_mb) * 100.0

# Process CPU usage relative to total system capacity (sysinfo process cpu / cores)
proc_cpu_relative = proc_cpu / num_cores

# ----------------------------------------------------
# PLOT SETUP: 3-Column layout
# ----------------------------------------------------
fig, (ax1, ax2, ax3) = plt.subplots(1, 3, figsize=(18, 6))

# 1) RTT Distribution Histogram (Centered)
bins = np.linspace(X_MIN, X_MAX, 50)
n_counts, _, _ = ax1.hist(rtt_plot, bins=bins, color="#5B92E5", edgecolor="black", alpha=0.8)
ax1.set_xlim(X_MIN, X_MAX)
# Set Y limit to leave 35% empty space at the top for the text box
ax1.set_ylim(0, np.max(n_counts) * 1.35 if len(n_counts) > 0 else 10)
ax1.set_xlabel("RTT [ms]")
ax1.set_ylabel("Frequency")
ax1.set_title("RTT Distribution")
ax1.grid(True, alpha=0.3)

# Add vertical lines for Mean, Median, P95 on the RTT plot
ax1.axvline(rtt_stats['mean'], color='red', linestyle='dashed', linewidth=2, label=f"Mean = {rtt_stats['mean']:.2f} ms")
ax1.axvline(rtt_stats['median'], color='green', linestyle='dashed', linewidth=2, label=f"Median = {rtt_stats['median']:.2f} ms")
ax1.axvline(rtt_stats['p95'], color='purple', linestyle='dashed', linewidth=2, label=f"P95 = {rtt_stats['p95']:.2f} ms")
# Move legend to upper left to prevent covering vertical lines
ax1.legend(loc="upper left")

# RTT Stats Box
text_rtt = (
    "RTT Statistics\n"
    f"N = {rtt_stats['N']}\n"
    f"Min = {rtt_stats['min']:.2f} ms\n"
    f"Max = {rtt_stats['max']:.2f} ms\n"
    f"Mean = {rtt_stats['mean']:.2f} ms\n"
    f"Median = {rtt_stats['median']:.2f} ms\n"
    f"P95 = {rtt_stats['p95']:.2f} ms"
)
ax1.text(
    0.95, 0.95, text_rtt, transform=ax1.transAxes, ha="right", va="top", fontsize=9,
    bbox=dict(facecolor="white", alpha=0.92, edgecolor="gray", boxstyle="round,pad=0.5")
)


# 2) Process CPU Usage (Global System Scale)
iterations = np.arange(len(proc_cpu_relative))
ax2.plot(iterations, proc_cpu_relative, color="#E28743", alpha=0.6, label="CPU Usage")
ax2.axhline(cpu_mean_total, color="red", linestyle="--", linewidth=1.5, label=f"Mean = {cpu_mean_total:.2f}%")
ax2.axhline(cpu_median_total, color="green", linestyle="--", linewidth=1.5, label=f"Median = {cpu_median_total:.2f}%")
# Set Y limit to leave 35% empty space at the top for the text box
max_cpu_val = np.max(proc_cpu_relative)
ax2.set_ylim(0, max(5.0, max_cpu_val * 1.35))
ax2.set_xlabel("Iteration")
ax2.set_ylabel("CPU Usage [% of system total]")
ax2.set_title("Process CPU Usage")
ax2.grid(True, alpha=0.3)
ax2.legend(loc="lower right")

# CPU Stats Box
text_cpu = (
    "CPU Metrics\n"
    f"Mean: {cpu_mean_total:.2f}% of system\n"
    f"  ({cpu_mean_core:.1f}% of core)\n"
    f"Median: {cpu_median_total:.2f}% of system\n"
    f"  ({cpu_median_core:.1f}% of core)"
)
ax2.text(
    0.95, 0.95, text_cpu, transform=ax2.transAxes, ha="right", va="top", fontsize=9,
    bbox=dict(facecolor="white", alpha=0.92, edgecolor="gray", boxstyle="round,pad=0.5")
)


# 3) Process RAM Usage
ax3.plot(iterations, proc_ram, color="#76B947", alpha=0.6, label="RAM Usage")
ax3.axhline(ram_mean_mb, color="red", linestyle="--", linewidth=1.5, label=f"Mean = {ram_mean_mb:.1f} MB")
ax3.axhline(ram_median_mb, color="green", linestyle="--", linewidth=1.5, label=f"Median = {ram_median_mb:.1f} MB")
# Set Y limit to leave 35% empty space at the top for the text box
min_ram_val = np.min(proc_ram)
max_ram_val = np.max(proc_ram)
ax3.set_ylim(max(0.0, min_ram_val - 2.0), max(max_ram_val + 5.0, max_ram_val * 1.35))
ax3.set_xlabel("Iteration")
ax3.set_ylabel("RAM Usage [MB]")
ax3.set_title("Process RAM Usage")
ax3.grid(True, alpha=0.3)
ax3.legend(loc="lower right")

# RAM Stats Box (system percentage first, absolute MB in brackets)
text_ram = (
    "RAM Metrics\n"
    f"Mean: {ram_mean_pct:.2f}% of system\n"
    f"  ({ram_mean_mb:.2f} MB)\n"
    f"Median: {ram_median_pct:.2f}% of system\n"
    f"  ({ram_median_mb:.2f} MB)"
)
ax3.text(
    0.95, 0.95, text_ram, transform=ax3.transAxes, ha="right", va="top", fontsize=9,
    bbox=dict(facecolor="white", alpha=0.92, edgecolor="gray", boxstyle="round,pad=0.5")
)

# Macro Title
fig.suptitle("Inter-container Communication (Same Host)", fontsize=16, fontweight="bold")
plt.tight_layout(rect=[0, 0, 1, 0.92])
plt.savefig("rtt_distribution.png", dpi=150)
print("[PLOT] Three-column distribution plot saved to rtt_distribution.png")
