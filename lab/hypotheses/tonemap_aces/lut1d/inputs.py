# Extra sampler images computed on the CPU from the scenario's uniforms (called by lab hypo): the per-channel curve LUT.
import numpy as np
def extra_inputs(u):
    n = 256
    t = (np.arange(n, dtype=np.float64) + 0.5) / n
    x = np.exp2(-10.0 + 18.0 * t)
    def curve(xc):
        c = np.power(np.maximum(xc, 0.0), u["contrast"])
        a, b, cc, d, e = 2.51, 0.03, 2.43, 0.59, 0.14
        y = np.clip((c * (a * c + b)) / (c * (cc * c + d) + e), 0.0, 1.0)
        return np.power(y, 1.0 / u["gamma"])
    img = np.zeros((1, n, 4), np.float32)
    img[0, :, 0] = curve(x); img[0, :, 1] = img[0, :, 0]; img[0, :, 2] = img[0, :, 0]; img[0, :, 3] = 1.0
    # NOTE: white_balance and exposure are applied in the shader before the log2 mapping, so one LUT serves all channels.
    return {"u_lut": img}
