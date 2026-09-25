# Extra uniform members computed on the CPU from the scenario's uniforms (called by lab hypo).
def extra_uniforms(u):
    warm = [1.0 + 0.1 * u["temperature"], 1.0, 1.0 - 0.1 * u["temperature"]]
    rows = {}
    for k in ("mat_r", "mat_g", "mat_b"):
        rows["h_" + k] = [u[k][i] * warm[i] for i in range(3)] + [0.0]
    rows["h_inv_gamma"] = [1.0 / max(u["gamma"][i], 1e-3) for i in range(3)] + [1.0]
    return rows
