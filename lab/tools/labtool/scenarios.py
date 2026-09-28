from __future__ import annotations
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from .paths import SCENARIOS
from .formats import FORMATS

@dataclass
class Pass:
    name: str
    shader: str
    samplers: dict
    uniforms: dict
    format: str
    scale: float
    load: str = "dont_care"
    store: str = "store"
    sampler: str = "linear"

@dataclass
class Scenario:
    name: str
    split: str
    width: int
    height: int
    inputs: list
    passes: list
    quality_outputs: list
    path: Path

    def pass_size(self, p: Pass) -> tuple[int, int]:
        return max(1, round(self.width * p.scale)), max(1, round(self.height * p.scale))

def load_scenario(name_or_path: str) -> Scenario:
    p = Path(name_or_path)
    if not p.exists():
        p = SCENARIOS / f"{name_or_path}.toml"
    d = tomllib.loads(p.read_text())
    s = d["scenario"]
    passes = [Pass(name=x["name"], shader=x["shader"], samplers=x.get("samplers", {}), uniforms=x.get("uniforms", {}),
                   format=x["output"]["format"], scale=float(x["output"].get("scale", 1.0)),
                   load=x.get("load", "dont_care"), store=x.get("store", "store"), sampler=x.get("sampler", "linear"))
              for x in d.get("passes", [])]
    q = d.get("quality", {}).get("outputs") or ([passes[-1].name] if passes else [])
    inputs = [{**i, "format": i.get("format", "RGBA32F")} for i in d.get("inputs", [])]
    for i in inputs:
        if i["format"] not in FORMATS:
            raise SystemExit(f"{p}: input '{i.get('name')}' format {i['format']!r} not in {FORMATS}")
    for x in passes:
        if x.format not in FORMATS:
            raise SystemExit(f"{p}: pass '{x.name}' output format {x.format!r} not in {FORMATS}")
    return Scenario(name=s["name"], split=s.get("split", "train"), width=int(s["width"]), height=int(s["height"]),
                    inputs=inputs, passes=passes, quality_outputs=q, path=p)

def all_scenarios(split: str | None = None) -> list[Scenario]:
    out = [load_scenario(str(p)) for p in sorted(SCENARIOS.glob("*.toml"))]
    return [s for s in out if split is None or s.split == split]
