//! Self-contained industrial thermometer report renderer.

use crate::report::Entry;
use crate::thermal::{
    format_value, normalize_entries, precision_for_step, ThermalProcess, ThermalState,
};
use std::cmp::Ordering;
use std::path::Path;

pub fn render_html(root: &Path, entries: &[Entry], title: Option<&str>) -> String {
    let processes = normalize_entries(entries);
    let title = title
        .map(str::to_string)
        .unwrap_or_else(|| format!("TempCheq Thermal Array — {}", root.display()));
    let array_mode = match processes.len() {
        0 => "empty",
        1 => "single",
        2 => "dual",
        3..=4 => "standard",
        5..=8 => "dense",
        _ => "wall",
    };
    let cards = if processes.is_empty() {
        empty_instrument()
    } else {
        processes
            .iter()
            .enumerate()
            .map(|(index, process)| render_instrument(process, index))
            .collect::<String>()
    };
    let summary = render_summary(&processes);

    let mut out = String::with_capacity(48_000 + cards.len());
    out.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    out.push_str("<meta name=\"color-scheme\" content=\"dark light\"><title>");
    out.push_str(&esc(&title));
    out.push_str("</title><style>");
    out.push_str(PANEL_CSS);
    out.push_str("</style></head><body><main class=\"console-shell\">");
    out.push_str("<header class=\"console-header\"><div class=\"brand-lockup\"><span class=\"brand-mark\" aria-hidden=\"true\"><i></i><i></i><i></i></span><div><p class=\"eyebrow\">INFERENCE THERMAL CONTROL / STATIC AUDIT</p><h1>");
    out.push_str(&esc(&title));
    out.push_str("</h1></div></div><div class=\"system-meta\"><span><b>SOURCE</b> ");
    out.push_str(&esc(&root.display().to_string()));
    out.push_str("</span><span><b>ENGINE</b> TempCheq v");
    out.push_str(env!("CARGO_PKG_VERSION"));
    out.push_str("</span><span class=\"online-flag\"><i></i> REPORT ONLINE</span></div></header>");
    out.push_str(&summary);
    out.push_str("<section class=\"control-deck\" aria-label=\"Instrument controls\"><div class=\"filter-bank\" role=\"group\" aria-label=\"Filter by state\"><button class=\"filter is-active\" data-filter=\"all\" aria-pressed=\"true\">ALL</button><button class=\"filter\" data-filter=\"optimal\" aria-pressed=\"false\">OPTIMAL</button><button class=\"filter\" data-filter=\"cold\" aria-pressed=\"false\">COLD</button><button class=\"filter\" data-filter=\"high\" aria-pressed=\"false\">HIGH</button><button class=\"filter\" data-filter=\"critical\" aria-pressed=\"false\">CRITICAL</button><button class=\"filter\" data-filter=\"unknown\" aria-pressed=\"false\">UNKNOWN</button></div><label class=\"search-control\"><span>FIND PROCESS</span><input id=\"process-search\" type=\"search\" autocomplete=\"off\" placeholder=\"name or location\"></label><label class=\"sort-control\"><span>ORDER</span><select id=\"process-sort\"><option value=\"severity\">INTERVENTION</option><option value=\"actual\">ACTUAL ↓</option><option value=\"deviation\">|Δ| ↓</option><option value=\"name\">PROCESS A–Z</option></select></label></section>");
    out.push_str("<section class=\"instrument-stage\" aria-labelledby=\"array-title\"><div class=\"stage-heading\"><div><p class=\"eyebrow\">THERMOMETER ARRAY</p><h2 id=\"array-title\">Process instrumentation</h2></div><p><span id=\"visible-count\">");
    out.push_str(&processes.len().to_string());
    out.push_str("</span> / ");
    out.push_str(&processes.len().to_string());
    out.push_str(" CHANNELS VISIBLE</p></div><div id=\"thermometer-array\" class=\"thermometer-array array--");
    out.push_str(array_mode);
    out.push_str("\" data-count=\"");
    out.push_str(&processes.len().to_string());
    out.push_str("\">");
    out.push_str(&cards);
    out.push_str("</div><div id=\"no-match\" class=\"no-match\" hidden>No instruments match the active filter.</div></section>");
    out.push_str("<footer class=\"console-footer\"><p><b>INTERPRETATION NOTE</b> Target and acceptable bands are TempCheq heuristic priors, not measured safety limits. Display bounds are presentation geometry only.</p><p>Sampling temperature is dimensionless; instruments therefore show <b>unitless</b> values. No freshness timestamp is shown because static audits do not supply one.</p></footer><div id=\"thermal-live\" class=\"sr-only\" aria-live=\"polite\"></div></main><script>");
    out.push_str(PANEL_JS);
    out.push_str("</script></body></html>");
    out
}

fn render_summary(processes: &[ThermalProcess]) -> String {
    let highest = processes
        .iter()
        .filter_map(|p| p.actual.map(|v| (p, v)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
    let largest = processes
        .iter()
        .filter_map(|p| p.deviation.map(|v| (p, v.abs())))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
    let optimal = processes
        .iter()
        .filter(|p| p.state == ThermalState::Optimal)
        .count();
    let warning = processes
        .iter()
        .filter(|p| matches!(p.state, ThermalState::Cold | ThermalState::High))
        .count();
    let critical = processes
        .iter()
        .filter(|p| p.state == ThermalState::Critical)
        .count();

    let highest_value = highest
        .map(|(p, value)| format!("{} · {}", p.name, format_value(value, p.precision)))
        .unwrap_or_else(|| "NO READING".to_string());
    let largest_value = largest
        .map(|(p, value)| format!("{} · {}", p.name, format_value(value, p.precision.max(2))))
        .unwrap_or_else(|| "NO DEVIATION".to_string());
    format!(
        "<section class=\"telemetry-strip\" aria-label=\"Array summary\"><div class=\"telemetry-primary\"><span>HIGHEST ACTUAL</span><strong>{}</strong></div><div class=\"telemetry-primary\"><span>LARGEST |Δ|</span><strong>{}</strong></div><div class=\"telemetry-count optimal\"><span>OPTIMAL</span><strong>{optimal:02}</strong></div><div class=\"telemetry-count warning\"><span>WARNING</span><strong>{warning:02}</strong></div><div class=\"telemetry-count critical\"><span>CRITICAL</span><strong>{critical:02}</strong></div></section>",
        esc(&highest_value),
        esc(&largest_value),
    )
}

fn render_instrument(process: &ThermalProcess, index: usize) -> String {
    let state = process.state.css_class();
    let actual = process.actual_text();
    let ideal = process.ideal_text();
    let deviation = process.deviation_text();
    let actual_pct = process.actual_percent();
    let ideal_pct = process.ideal_percent();
    let range_pct = process.range_percent();
    let tick_precision = precision_for_step(process.scale.major_step);
    let ticks = process
        .scale
        .ticks
        .iter()
        .map(|tick| {
            if tick.major {
                format!(
                    "<i class=\"tick tick--major\" style=\"--tick:{:.3}%\"><span>{}</span></i>",
                    tick.percent,
                    format_value(tick.value, tick_precision)
                )
            } else {
                format!(
                    "<i class=\"tick tick--minor\" style=\"--tick:{:.3}%\"></i>",
                    tick.percent
                )
            }
        })
        .collect::<String>();
    let range = range_pct
        .map(|(low, high)| {
            format!(
                "<div class=\"operating-band\" style=\"--band-low:{low:.3}%;--band-size:{:.3}%\" aria-hidden=\"true\"></div>",
                (high - low).max(0.0)
            )
        })
        .unwrap_or_default();
    let ideal_marker = ideal_pct
        .map(|percent| {
            format!(
                "<div class=\"ideal-marker\" style=\"--ideal:{percent:.3}%\"><span>TARGET {}</span></div>",
                esc(&ideal)
            )
        })
        .unwrap_or_default();
    let actual_marker = process
        .actual
        .map(|_| {
            format!(
                "<div class=\"actual-marker\" style=\"--actual:{actual_pct:.3}%\"><span>ACTUAL</span></div>"
            )
        })
        .unwrap_or_default();
    let reasons = process
        .reasons
        .iter()
        .map(|reason| format!("<li>{}</li>", esc(reason)))
        .collect::<String>();
    let actual_data = process.actual.map(|v| v.to_string()).unwrap_or_default();
    let ideal_data = process.ideal.map(|v| v.to_string()).unwrap_or_default();
    let low_data = process
        .operating_min
        .map(|v| v.to_string())
        .unwrap_or_default();
    let high_data = process
        .operating_max
        .map(|v| v.to_string())
        .unwrap_or_default();
    let critical_data = process
        .critical_high
        .map(|v| v.to_string())
        .unwrap_or_default();
    let reading_label = if process.actual.is_some() {
        format!("{} {}", actual, process.unit)
    } else if process.ideal.is_none() {
        "NOT APPLICABLE".to_string()
    } else {
        "NO EXPLICIT VALUE".to_string()
    };
    let freshness = process
        .freshness
        .as_deref()
        .unwrap_or("STATIC AUDIT · FRESHNESS N/A");
    let aria = format!(
        "{}. State {}. Actual {}. Target {}. Estimated operating band {} to {}. Deviation {}.",
        process.name,
        process.state.label(),
        reading_label,
        ideal,
        process
            .operating_min
            .map(|v| format_value(v, 2))
            .unwrap_or_else(|| "unavailable".to_string()),
        process
            .operating_max
            .map(|v| format_value(v, 2))
            .unwrap_or_else(|| "unavailable".to_string()),
        deviation
    );

    format!(
        r#"<article class="thermometer state-{state}" tabindex="0" role="group" aria-label="{aria}" data-id="{id}" data-name="{name_data}" data-location="{location_data}" data-state="{state}" data-severity="{severity}" data-actual="{actual_data}" data-ideal="{ideal_data}" data-low="{low_data}" data-high="{high_data}" data-critical="{critical_data}" data-deviation="{deviation_data}" data-scale-min="{scale_min}" data-scale-max="{scale_max}" data-precision="{precision}" style="--target-fill:{actual_pct:.3}%;--delay:{delay}ms">
<header class="instrument-header"><div><p class="channel">CHANNEL {channel:02}</p><h3>{name}</h3><p class="process-id">{location}</p></div><span class="state-flag"><i aria-hidden="true"></i>{state_label}</span></header>
<div class="readout"><div class="readout-value"><span class="actual-number" data-value="{actual_data}">{actual}</span><small>{unit}</small></div><div class="delta"><span>Δ TARGET</span><strong>{deviation}</strong></div></div>
<div class="instrument-core">
  <div class="scale-zone" aria-hidden="true"><div class="ticks">{ticks}</div><div class="scale-caption"><span>DISPLAY MAX {display_max}</span><span>DISPLAY MIN {display_min}</span></div></div>
  <div class="tube-zone">{range}<div class="tube"><div class="tube-depth"></div><div class="fluid {fluid_state}" style="--fill:{actual_pct:.3}%"><i></i></div></div><div class="bulb"><i></i></div>{ideal_marker}{actual_marker}</div>
  <div class="reference-rail"><div><span>ACTUAL</span><strong class="rail-actual">{reading_label}</strong></div><div><span>TARGET</span><strong>{ideal}</strong></div><div><span>EST. BAND</span><strong>{band}</strong></div><div><span>CONFIDENCE</span><strong>{confidence:.0}%</strong></div></div>
</div>
<div class="instrument-status"><span class="direction" aria-hidden="true">{direction}</span><div><b>{state_label}</b><span>{state_detail}</span></div><span class="freshness">{freshness}</span></div>
<details class="instrument-details"><summary>PROCESS DETAIL <span>↗</span></summary><dl><div><dt>Source kind</dt><dd>{source_kind}</dd></div><div><dt>Task class</dt><dd>{task_class}</dd></div><div><dt>Identifier</dt><dd>{id}</dd></div><div><dt>Critical threshold</dt><dd>Not supplied</dd></div></dl><ul>{reasons}</ul></details>
</article>"#,
        state = state,
        aria = esc(&aria),
        id = esc(&process.id),
        name_data = esc(&process.name.to_lowercase()),
        location_data = esc(&process.location.to_lowercase()),
        severity = process.state.rank(),
        actual_data = actual_data,
        ideal_data = ideal_data,
        low_data = low_data,
        high_data = high_data,
        critical_data = critical_data,
        deviation_data = process.deviation.map(f32::abs).unwrap_or(-1.0),
        scale_min = process.scale.display_min,
        scale_max = process.scale.display_max,
        precision = process.precision,
        delay = (index % 8) * 55,
        channel = index + 1,
        name = esc(&process.name),
        location = esc(&process.location),
        state_label = process.state.label(),
        actual = esc(&actual),
        unit = process.unit,
        deviation = esc(&deviation),
        actual_pct = actual_pct,
        ticks = ticks,
        display_max = format_value(process.scale.display_max, tick_precision),
        display_min = format_value(process.scale.display_min, tick_precision),
        range = range,
        fluid_state = if process.actual.is_some() {
            ""
        } else {
            "fluid--offline"
        },
        ideal_marker = ideal_marker,
        actual_marker = actual_marker,
        reading_label = esc(&reading_label),
        ideal = esc(&ideal),
        band = match (process.operating_min, process.operating_max) {
            (Some(low), Some(high)) =>
                format!("{}—{}", format_value(low, 2), format_value(high, 2)),
            _ => "UNAVAILABLE".to_string(),
        },
        confidence = process.confidence * 100.0,
        direction = match process.state {
            ThermalState::Cold => "↓",
            ThermalState::High | ThermalState::Critical => "↑",
            ThermalState::Optimal => "◆",
            ThermalState::Unknown => "—",
        },
        state_detail = match process.state {
            ThermalState::Cold => "BELOW ESTIMATED OPERATING BAND",
            ThermalState::Optimal => "WITHIN ESTIMATED OPERATING BAND",
            ThermalState::High => "ABOVE ESTIMATED OPERATING BAND",
            ThermalState::Critical => "AUTHORITATIVE CRITICAL LIMIT EXCEEDED",
            ThermalState::Unknown => "ACTUAL READING UNAVAILABLE",
        },
        freshness = esc(freshness),
        source_kind = esc(&process.source_kind),
        task_class = esc(&process.task_class),
        reasons = reasons,
    )
}

fn empty_instrument() -> String {
    "<article class=\"thermometer thermometer--empty state-unknown\" role=\"status\"><header class=\"instrument-header\"><div><p class=\"channel\">CHANNEL —</p><h3>NO INFERENCE PROCESSES</h3><p class=\"process-id\">DISCOVERY RETURNED ZERO ACTIONS</p></div><span class=\"state-flag\"><i></i>OFFLINE</span></header><div class=\"empty-core\"><div class=\"offline-tube\"><i></i></div><p>INSTRUMENT ARRAY STANDBY</p><span>Run TempCheq against a workspace containing a recognized inference action.</span></div></article>".to_string()
}

fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const PANEL_CSS: &str = r#"
:root{color-scheme:dark;--bg:#070b0e;--bg-grid:rgba(135,181,190,.045);--panel:#0d1418;--panel-2:#111b20;--line:#26383f;--line-hot:#47616a;--text:#eef5f4;--muted:#7f969c;--dim:#52676d;--cyan:#65d7df;--optimal:#65dfb0;--cold:#73b9ee;--high:#f1b866;--critical:#ff6b62;--unknown:#8a989c;--shadow:0 24px 70px rgba(0,0,0,.34)}
*{box-sizing:border-box}html{background:var(--bg)}body{margin:0;min-width:300px;background:radial-gradient(circle at 12% 0,rgba(55,111,119,.12),transparent 34rem),linear-gradient(var(--bg-grid) 1px,transparent 1px),linear-gradient(90deg,var(--bg-grid) 1px,transparent 1px),var(--bg);background-size:auto,28px 28px,28px 28px;color:var(--text);font-family:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;font-variant-numeric:tabular-nums;line-height:1.35}.console-shell{width:min(1680px,100%);margin:auto;padding:28px clamp(16px,3vw,48px) 44px}.console-header{display:flex;align-items:flex-start;justify-content:space-between;gap:28px;padding:8px 0 24px;border-bottom:1px solid var(--line)}.brand-lockup{display:flex;gap:16px;align-items:center}.brand-mark{position:relative;display:flex;align-items:flex-end;gap:3px;width:34px;height:42px;padding:6px;border:1px solid var(--line-hot);background:#0a1115}.brand-mark:after{content:"";position:absolute;inset:3px;border:1px solid rgba(101,215,223,.15)}.brand-mark i{display:block;flex:1;background:var(--cyan);box-shadow:0 0 10px rgba(101,215,223,.5)}.brand-mark i:nth-child(1){height:35%}.brand-mark i:nth-child(2){height:72%}.brand-mark i:nth-child(3){height:52%}.eyebrow,.channel{margin:0 0 5px;color:var(--cyan);font:600 10px/1.2 ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.18em;text-transform:uppercase}.console-header h1{max-width:880px;margin:0;font-size:clamp(20px,2.2vw,34px);font-weight:580;letter-spacing:-.035em}.system-meta{display:grid;gap:5px;text-align:right;color:var(--muted);font:500 10px/1.45 ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.06em}.system-meta b{color:var(--dim);font-weight:700}.online-flag{color:var(--optimal)!important}.online-flag i{display:inline-block;width:6px;height:6px;margin-right:6px;border-radius:50%;background:currentColor;box-shadow:0 0 9px currentColor}.telemetry-strip{display:grid;grid-template-columns:minmax(220px,1.5fr) minmax(220px,1.5fr) repeat(3,minmax(100px,.55fr));margin:18px 0;border:1px solid var(--line);background:rgba(10,16,19,.82);box-shadow:var(--shadow)}.telemetry-strip>div{min-height:72px;padding:13px 16px;border-right:1px solid var(--line);display:flex;flex-direction:column;justify-content:space-between}.telemetry-strip>div:last-child{border-right:0}.telemetry-strip span{color:var(--muted);font:600 9px/1.2 ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.14em}.telemetry-strip strong{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:14px;font-weight:600}.telemetry-count strong{font:500 28px/1 ui-monospace,SFMono-Regular,Consolas,monospace}.telemetry-count.optimal strong{color:var(--optimal)}.telemetry-count.warning strong{color:var(--high)}.telemetry-count.critical strong{color:var(--critical)}.control-deck{display:flex;align-items:end;gap:14px;margin:18px 0 26px;padding:12px;border:1px solid var(--line);background:rgba(13,20,24,.92)}.filter-bank{display:flex;flex-wrap:wrap;gap:5px;margin-right:auto}.filter{min-height:34px;padding:0 12px;border:1px solid transparent;background:transparent;color:var(--muted);font:700 9px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.11em;cursor:pointer}.filter:hover,.filter:focus-visible{border-color:var(--line-hot);color:var(--text)}.filter.is-active{border-color:var(--cyan);background:rgba(101,215,223,.08);color:var(--cyan)}.search-control,.sort-control{display:grid;gap:5px;color:var(--dim);font:700 8px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.14em}.search-control input,.sort-control select{height:34px;border:1px solid var(--line);border-radius:0;background:#091014;color:var(--text);padding:0 10px;font:500 11px ui-monospace,SFMono-Regular,Consolas,monospace;outline:0}.search-control input{width:min(230px,28vw)}.sort-control select{width:145px}.search-control input:focus,.sort-control select:focus{border-color:var(--cyan);box-shadow:0 0 0 2px rgba(101,215,223,.12)}.instrument-stage{margin-top:18px}.stage-heading{display:flex;justify-content:space-between;align-items:end;gap:20px;margin-bottom:14px}.stage-heading h2{margin:0;font-size:18px;font-weight:560}.stage-heading>p{margin:0;color:var(--dim);font:600 9px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.12em}.thermometer-array{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,320px),1fr));gap:14px;align-items:start}.array--single{grid-template-columns:minmax(min(100%,460px),700px);justify-content:center}.array--dual{grid-template-columns:repeat(2,minmax(300px,580px));justify-content:center}.array--wall{max-height:calc(100vh - 190px);min-height:540px;overflow:auto;padding-right:7px;scrollbar-color:var(--line-hot) transparent}.thermometer{--state:var(--unknown);position:relative;isolation:isolate;min-width:0;border:1px solid var(--line);background:linear-gradient(145deg,rgba(255,255,255,.025),transparent 34%),var(--panel);box-shadow:0 14px 36px rgba(0,0,0,.2);transition:border-color .25s,transform .25s,opacity .2s;animation:instrument-online .5s both;animation-delay:var(--delay)}.thermometer:before{content:"";position:absolute;z-index:-1;inset:4px;border:1px solid rgba(255,255,255,.025);pointer-events:none}.thermometer:hover,.thermometer:focus-visible,.thermometer.is-selected{border-color:color-mix(in srgb,var(--state) 60%,var(--line));transform:translateY(-2px);outline:0}.thermometer[hidden]{display:none}.state-cold{--state:var(--cold)}.state-optimal{--state:var(--optimal)}.state-high{--state:var(--high)}.state-critical{--state:var(--critical)}.state-unknown{--state:var(--unknown)}.instrument-header{display:flex;justify-content:space-between;gap:16px;min-height:80px;padding:15px 16px 13px;border-bottom:1px solid var(--line)}.instrument-header h3{max-width:25ch;margin:0 0 5px;overflow-wrap:anywhere;font-size:16px;font-weight:620;letter-spacing:-.02em}.process-id{max-width:35ch;margin:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--dim);font:500 9px ui-monospace,SFMono-Regular,Consolas,monospace}.state-flag{height:27px;display:inline-flex;align-items:center;gap:7px;padding:0 9px;border:1px solid color-mix(in srgb,var(--state) 52%,var(--line));color:var(--state);font:800 9px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.1em}.state-flag i{width:7px;height:7px;border:1px solid currentColor;transform:rotate(45deg);background:color-mix(in srgb,var(--state) 35%,transparent)}.readout{display:flex;align-items:center;justify-content:space-between;gap:18px;padding:15px 16px 5px}.readout-value{display:flex;align-items:baseline;gap:7px;min-width:0}.actual-number{color:var(--text);font:500 clamp(36px,4vw,54px)/.95 ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:-.075em;text-shadow:0 0 24px color-mix(in srgb,var(--state) 22%,transparent)}.readout-value small{color:var(--muted);font:700 9px ui-monospace,SFMono-Regular,Consolas,monospace;text-transform:uppercase;letter-spacing:.1em}.delta{display:grid;text-align:right}.delta span,.reference-rail span{color:var(--dim);font:700 8px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.13em}.delta strong{color:var(--state);font:600 17px ui-monospace,SFMono-Regular,Consolas,monospace}.instrument-core{display:grid;grid-template-columns:minmax(90px,.8fr) minmax(105px,1fr) minmax(105px,.95fr);gap:5px;min-height:350px;padding:12px 14px 18px}.scale-zone{position:relative;height:300px}.ticks{position:absolute;inset:10px 0 10px 0}.tick{position:absolute;right:0;bottom:var(--tick);display:block;width:28px;border-top:1px solid var(--dim)}.tick--major{width:37px;border-color:var(--muted)}.tick--major span{position:absolute;right:43px;top:-7px;color:var(--muted);font:500 9px ui-monospace,SFMono-Regular,Consolas,monospace}.tick--minor{width:18px;opacity:.58}.scale-caption{position:absolute;inset:0 auto 0 0;display:flex;flex-direction:column;justify-content:space-between;color:var(--dim);font:600 7px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.08em;writing-mode:vertical-rl;transform:rotate(180deg)}.tube-zone{position:relative;height:330px}.tube{position:absolute;z-index:2;left:50%;top:8px;bottom:47px;width:34px;transform:translateX(-50%);overflow:hidden;border:2px solid #52666c;border-bottom:0;border-radius:18px 18px 5px 5px;background:linear-gradient(90deg,#05090b 0,#142027 44%,#091014 72%,#020506 100%);box-shadow:inset 0 0 0 4px rgba(255,255,255,.025),0 0 0 1px #030506,0 0 25px rgba(0,0,0,.5)}.tube-depth{position:absolute;z-index:3;inset:4px auto 0 7px;width:5px;border-radius:4px;background:linear-gradient(rgba(255,255,255,.17),transparent 50%);pointer-events:none}.fluid{position:absolute;z-index:2;left:5px;right:5px;bottom:0;height:var(--fill);min-height:3px;border-radius:10px 10px 0 0;background:linear-gradient(90deg,color-mix(in srgb,var(--state) 55%,#10232a),var(--state) 58%,#f4ffff 72%,var(--state) 89%);box-shadow:0 -5px 14px color-mix(in srgb,var(--state) 60%,transparent),inset 0 0 8px rgba(255,255,255,.25);transition:height 1.15s cubic-bezier(.2,.82,.22,1);will-change:height}.fluid i{position:absolute;left:2px;right:2px;top:-2px;height:5px;border-radius:50%;background:#eefeff;box-shadow:0 0 9px var(--state)}.fluid--offline{height:0!important;min-height:0;background:transparent;box-shadow:none}.bulb{position:absolute;z-index:3;left:50%;bottom:10px;width:72px;height:72px;transform:translateX(-50%);border:2px solid #52666c;border-radius:50%;background:radial-gradient(circle at 42% 34%,#ecffff 0 4%,var(--state) 9%,color-mix(in srgb,var(--state) 65%,#091014) 48%,#05090b 74%);box-shadow:inset -9px -10px 16px rgba(0,0,0,.38),0 0 25px color-mix(in srgb,var(--state) 22%,transparent),0 0 0 1px #030506;transition:background .3s}.fluid--offline+.bulb,.state-unknown .bulb{filter:saturate(.25);opacity:.65}.bulb i{position:absolute;left:17px;top:12px;width:13px;height:8px;border-radius:50%;background:rgba(255,255,255,.4);transform:rotate(-32deg)}.operating-band{position:absolute;z-index:0;left:7px;right:7px;bottom:calc(47px + (283px * var(--band-low) / 100));height:calc(283px * var(--band-size) / 100);border-top:1px solid rgba(101,223,176,.36);border-bottom:1px solid rgba(101,223,176,.36);background:repeating-linear-gradient(135deg,rgba(101,223,176,.09) 0 4px,transparent 4px 9px)}.ideal-marker,.actual-marker{position:absolute;z-index:5;left:3px;right:3px;bottom:calc(47px + (283px * var(--ideal) / 100));border-top:2px solid var(--cyan);pointer-events:none}.ideal-marker:before{content:"";position:absolute;left:50%;top:-5px;width:8px;height:8px;border:2px solid var(--cyan);background:var(--panel);transform:translateX(-50%) rotate(45deg)}.ideal-marker span{position:absolute;left:calc(50% + 28px);top:-8px;padding:2px 5px;border:1px solid rgba(101,215,223,.36);background:var(--panel);color:var(--cyan);white-space:nowrap;font:700 7px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.08em}.actual-marker{bottom:calc(47px + (283px * var(--actual) / 100));right:calc(50% + 22px);left:4px;border-color:var(--state);border-style:dashed}.actual-marker span{position:absolute;right:calc(100% + 4px);top:-7px;color:var(--state);font:800 7px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.07em}.reference-rail{display:grid;align-content:center;gap:15px;padding-left:8px}.reference-rail>div{display:grid;gap:4px;padding:0 0 10px;border-bottom:1px solid var(--line)}.reference-rail strong{overflow-wrap:anywhere;font:600 11px/1.25 ui-monospace,SFMono-Regular,Consolas,monospace}.reference-rail>div:first-child strong{color:var(--state)}.instrument-status{display:grid;grid-template-columns:34px 1fr auto;align-items:center;gap:10px;min-height:58px;padding:10px 14px;border-top:1px solid var(--line);background:color-mix(in srgb,var(--state) 5%,transparent)}.direction{display:grid;place-items:center;width:30px;height:30px;border:1px solid color-mix(in srgb,var(--state) 50%,var(--line));color:var(--state);font:700 17px ui-monospace,SFMono-Regular,Consolas,monospace}.instrument-status>div{display:grid}.instrument-status b{color:var(--state);font:800 9px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.13em}.instrument-status div span,.freshness{color:var(--dim);font:600 7px/1.5 ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.07em}.freshness{text-align:right}.instrument-details{border-top:1px solid var(--line)}.instrument-details summary{display:flex;justify-content:space-between;padding:10px 14px;color:var(--muted);font:700 8px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.12em;cursor:pointer;list-style:none}.instrument-details summary::-webkit-details-marker{display:none}.instrument-details[open] summary{color:var(--cyan)}.instrument-details dl{display:grid;grid-template-columns:1fr 1fr;gap:1px;margin:0;padding:0 14px;background:var(--line)}.instrument-details dl div{min-width:0;padding:9px;background:var(--panel)}.instrument-details dt{color:var(--dim);font:700 7px ui-monospace,SFMono-Regular,Consolas,monospace;text-transform:uppercase}.instrument-details dd{margin:3px 0 0;overflow-wrap:anywhere;font:500 9px ui-monospace,SFMono-Regular,Consolas,monospace}.instrument-details ul{margin:0;padding:12px 28px 14px;color:var(--muted);font-size:11px}.state-optimal .state-flag i{border-radius:50%;transform:none}.state-high .state-flag i{transform:rotate(45deg);border-radius:0}.state-critical{border-color:rgba(255,107,98,.52)}.state-critical .state-flag,.state-critical .direction{animation:critical-pulse 1.8s ease-in-out infinite}.state-unknown .tube{background:repeating-linear-gradient(135deg,#091014 0 8px,#0c171b 8px 16px)}.thermometer--empty{grid-column:1/-1;max-width:640px;width:100%;margin:auto}.empty-core{min-height:410px;display:grid;place-items:center;align-content:center;gap:12px;text-align:center}.offline-tube{position:relative;width:40px;height:230px;margin-bottom:28px;border:2px solid var(--dim);border-radius:22px 22px 5px 5px;background:repeating-linear-gradient(135deg,#091014 0 9px,#101c21 9px 18px)}.offline-tube:after{content:"";position:absolute;left:50%;bottom:-40px;width:78px;height:78px;transform:translateX(-50%);border:2px solid var(--dim);border-radius:50%;background:#0b1317}.offline-tube i{position:absolute;inset:8px;border-left:3px solid rgba(255,255,255,.05)}.empty-core p{margin:20px 0 0;color:var(--unknown);font:800 11px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.16em}.empty-core span{max-width:420px;color:var(--muted);font-size:12px}.no-match{padding:40px;border:1px dashed var(--line-hot);color:var(--muted);text-align:center;font:700 10px ui-monospace,SFMono-Regular,Consolas,monospace;letter-spacing:.12em}.console-footer{display:grid;grid-template-columns:1fr 1fr;gap:20px;margin-top:28px;padding-top:18px;border-top:1px solid var(--line);color:var(--muted);font:500 10px/1.6 ui-monospace,SFMono-Regular,Consolas,monospace}.console-footer p{margin:0}.console-footer b{color:var(--text)}.sr-only{position:absolute!important;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}@keyframes instrument-online{from{opacity:0;transform:translateY(10px)}to{opacity:1;transform:none}}@keyframes critical-pulse{0%,100%{box-shadow:0 0 0 transparent}50%{box-shadow:0 0 18px rgba(255,107,98,.35)}}
@media(max-width:900px){.console-header{display:grid}.system-meta{text-align:left}.telemetry-strip{grid-template-columns:1fr 1fr}.telemetry-strip>div{border-bottom:1px solid var(--line)}.telemetry-strip>div:nth-child(2n){border-right:0}.control-deck{align-items:stretch;flex-wrap:wrap}.filter-bank{width:100%}.search-control{flex:1}.search-control input{width:100%}.array--dual{grid-template-columns:1fr}.console-footer{grid-template-columns:1fr}}
@media(max-width:560px){.console-shell{padding:18px 10px 32px}.brand-mark{display:none}.telemetry-strip{grid-template-columns:1fr 1fr}.telemetry-primary{grid-column:1/-1}.control-deck{padding:9px}.filter{padding:0 8px}.search-control,.sort-control{width:100%}.search-control input,.sort-control select{width:100%}.stage-heading>p{display:none}.thermometer-array{grid-template-columns:1fr}.instrument-core{grid-template-columns:78px minmax(100px,1fr)}.reference-rail{grid-column:1/-1;grid-template-columns:repeat(2,1fr);padding:0}.reference-rail>div{padding:8px}.instrument-status{grid-template-columns:30px 1fr}.freshness{display:none}.actual-number{font-size:39px}.ideal-marker span{left:calc(50% + 20px)}.console-footer{font-size:9px}}
@media(prefers-color-scheme:light){:root{color-scheme:light;--bg:#e7edef;--bg-grid:rgba(25,56,64,.065);--panel:#f8fbfb;--panel-2:#eef4f5;--line:#bac8cc;--line-hot:#799096;--text:#102126;--muted:#4d666d;--dim:#698087;--cyan:#007b88;--optimal:#087b5d;--cold:#156ea5;--high:#a85d00;--critical:#b52e27;--unknown:#607176;--shadow:0 18px 50px rgba(24,53,60,.13)}body{background:linear-gradient(var(--bg-grid) 1px,transparent 1px),linear-gradient(90deg,var(--bg-grid) 1px,transparent 1px),var(--bg);background-size:28px 28px}.search-control input,.sort-control select{background:#fff}.tube{background:linear-gradient(90deg,#9eafb3,#edf5f6 46%,#bdcdd0 74%,#87999e)}.ideal-marker span{background:var(--panel)}}
@media(prefers-reduced-motion:reduce){*,*:before,*:after{scroll-behavior:auto!important;animation-delay:0ms!important;animation-duration:.001ms!important;animation-iteration-count:1!important;transition-duration:.001ms!important}}
@media print{body{background:#fff;color:#111}.console-shell{max-width:none;padding:12px}.control-deck{display:none}.thermometer-array{grid-template-columns:repeat(2,1fr)}.thermometer{break-inside:avoid;box-shadow:none}.instrument-details{display:none}.array--wall{max-height:none;overflow:visible}}
"#;

const PANEL_JS: &str = r#"
(()=>{'use strict';const array=document.getElementById('thermometer-array');if(!array)return;const cards=()=>[...array.querySelectorAll('.thermometer[data-id]')];const live=document.getElementById('thermal-live');const reduced=matchMedia('(prefers-reduced-motion: reduce)').matches;const finite=v=>v!==''&&v!==null&&v!==undefined&&Number.isFinite(Number(v));const clamp=(v,a,b)=>Math.min(b,Math.max(a,v));const stateFor=(actual,low,high,critical)=>{if(!finite(actual))return'unknown';actual=Number(actual);if(finite(critical)&&actual>=Number(critical))return'critical';if(finite(low)&&actual<Number(low))return'cold';if(finite(high)&&actual>Number(high))return'high';return finite(low)||finite(high)?'optimal':'unknown'};const stateLabel=s=>({cold:'COLD',optimal:'OPTIMAL',high:'HIGH',critical:'CRITICAL',unknown:'UNKNOWN'}[s]||'UNKNOWN');
function animateCard(card){const fluid=card.querySelector('.fluid:not(.fluid--offline)');if(fluid){const target=card.style.getPropertyValue('--target-fill')||'0%';fluid.style.setProperty('--fill',reduced?target:'0%');requestAnimationFrame(()=>requestAnimationFrame(()=>fluid.style.setProperty('--fill',target)))}const number=card.querySelector('.actual-number');if(!number||!finite(number.dataset.value)||reduced)return;const end=Number(number.dataset.value),precision=Number(card.dataset.precision||2),duration=650,start=performance.now();number.textContent=(0).toFixed(precision);const frame=now=>{const t=clamp((now-start)/duration,0,1),eased=1-Math.pow(1-t,3);number.textContent=(end*eased).toFixed(precision);if(t<1)requestAnimationFrame(frame)};requestAnimationFrame(frame)}
function applyView(){const filter=document.querySelector('.filter.is-active')?.dataset.filter||'all',term=(document.getElementById('process-search')?.value||'').trim().toLowerCase();let visible=0;cards().forEach(card=>{const state=card.dataset.state,haystack=`${card.dataset.name} ${card.dataset.location}`,show=(filter==='all'||filter===state)&&(!term||haystack.includes(term));card.hidden=!show;if(show)visible++});document.getElementById('visible-count').textContent=visible;document.getElementById('no-match').hidden=visible!==0}
function sortCards(mode){const sorted=cards().sort((a,b)=>{if(mode==='name')return a.dataset.name.localeCompare(b.dataset.name);if(mode==='actual')return (Number(b.dataset.actual)||-Infinity)-(Number(a.dataset.actual)||-Infinity);if(mode==='deviation')return Number(b.dataset.deviation)-Number(a.dataset.deviation);return Number(b.dataset.severity)-Number(a.dataset.severity)||Number(b.dataset.deviation)-Number(a.dataset.deviation)||a.dataset.name.localeCompare(b.dataset.name)});sorted.forEach(card=>array.appendChild(card));applyView()}
document.querySelectorAll('.filter').forEach(button=>button.addEventListener('click',()=>{document.querySelectorAll('.filter').forEach(item=>{const active=item===button;item.classList.toggle('is-active',active);item.setAttribute('aria-pressed',active)});applyView()}));document.getElementById('process-search')?.addEventListener('input',applyView);document.getElementById('process-sort')?.addEventListener('change',event=>sortCards(event.target.value));cards().forEach(card=>{animateCard(card);card.addEventListener('click',event=>{if(event.target.closest('details,summary,a,button,input,select'))return;card.classList.toggle('is-selected')});card.addEventListener('keydown',event=>{if((event.key==='Enter'||event.key===' ')&&!event.target.closest('summary')){event.preventDefault();card.classList.toggle('is-selected')}})});
function update(id,actual){const card=cards().find(item=>item.dataset.id===id);if(!card)return false;const value=Number(actual),min=Number(card.dataset.scaleMin),max=Number(card.dataset.scaleMax),precision=Number(card.dataset.precision||2),number=card.querySelector('.actual-number'),fluid=card.querySelector('.fluid');if(!Number.isFinite(value)){card.dataset.actual='';setState(card,'unknown');number.textContent='—';fluid?.classList.add('fluid--offline');card.querySelector('.actual-marker')?.remove();announce(`${card.dataset.name} reading unavailable`);return true}const pct=clamp((value-min)/(max-min)*100,0,100);card.dataset.actual=String(value);card.style.setProperty('--target-fill',`${pct}%`);card.classList.toggle('is-outside-scale',value<min||value>max);fluid?.classList.remove('fluid--offline');let marker=card.querySelector('.actual-marker');if(!marker){marker=document.createElement('div');marker.className='actual-marker';marker.innerHTML='<span>ACTUAL</span>';card.querySelector('.tube-zone')?.appendChild(marker)}marker.style.setProperty('--actual',`${pct}%`);const ideal=finite(card.dataset.ideal)?Number(card.dataset.ideal):NaN,delta=Number.isFinite(ideal)?value-ideal:NaN;card.dataset.deviation=Number.isFinite(delta)?String(Math.abs(delta)):'-1';number.dataset.value=String(value);number.textContent=value.toFixed(precision);const rail=card.querySelector('.rail-actual');if(rail)rail.textContent=`${value.toFixed(precision)} unitless`;const deltaNode=card.querySelector('.delta strong');if(deltaNode)deltaNode.textContent=Number.isFinite(delta)?`${delta>=0?'+':''}${delta.toFixed(Math.max(2,precision))}`:'—';setState(card,stateFor(value,card.dataset.low,card.dataset.high,card.dataset.critical));animateCard(card);announce(`${card.dataset.name} updated to ${value.toFixed(precision)}, ${stateLabel(card.dataset.state)}`);return true}
function setState(card,state){['cold','optimal','high','critical','unknown'].forEach(s=>card.classList.remove(`state-${s}`));card.classList.add(`state-${state}`);card.dataset.state=state;card.dataset.severity=String({critical:4,high:3,cold:2,unknown:1,optimal:0}[state]);const flag=card.querySelector('.state-flag');if(flag)flag.lastChild.textContent=stateLabel(state)}function announce(message){live.textContent='';requestAnimationFrame(()=>live.textContent=message)}
window.TempCheqThermal=Object.freeze({update,refresh:()=>cards().forEach(animateCard),filter:applyView,sort:sortCards});applyView()})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thermal::{calculate_scale, ThermalProcess};

    fn process(state: ThermalState, actual: Option<f32>) -> ThermalProcess {
        ThermalProcess {
            id: "src/demo.rs:42".into(),
            name: "answer<unsafe>".into(),
            location: "src/demo.rs:42".into(),
            source_kind: "api-call".into(),
            task_class: "conversational".into(),
            actual,
            ideal: Some(0.7),
            operating_min: Some(0.5),
            operating_max: Some(0.8),
            critical_high: None,
            deviation: actual.map(|value| value - 0.7),
            state,
            unit: "unitless",
            precision: 1,
            confidence: 0.8,
            freshness: None,
            scale: calculate_scale(actual, Some(0.7), Some(0.5), Some(0.8), None),
            reasons: vec!["test reason".into()],
        }
    }

    #[test]
    fn instrument_is_accessible_and_escapes_process_identity() {
        let html = render_instrument(&process(ThermalState::Optimal, Some(0.7)), 0);
        assert!(html.contains("role=\"group\""));
        assert!(html.contains("tabindex=\"0\""));
        assert!(html.contains("ideal-marker"));
        assert!(html.contains("actual-marker"));
        assert!(html.contains("answer&lt;unsafe&gt;"));
        assert!(!html.contains("answer<unsafe>"));
    }

    #[test]
    fn unavailable_reading_keeps_target_but_removes_actual_marker() {
        let html = render_instrument(&process(ThermalState::Unknown, None), 0);
        assert!(html.contains("ideal-marker"));
        assert!(!html.contains("class=\"actual-marker\""));
        assert!(html.contains("NO EXPLICIT VALUE"));
    }

    #[test]
    fn empty_report_renders_an_offline_instrument() {
        let html = render_html(Path::new("/workspace"), &[], None);
        assert!(html.contains("NO INFERENCE PROCESSES"));
        assert!(html.contains("TempCheqThermal"));
        assert!(html.contains("prefers-reduced-motion"));
    }

    #[test]
    fn live_controller_can_promote_offline_instruments() {
        assert!(PANEL_JS.contains("classList.remove('fluid--offline')"));
        assert!(PANEL_JS.contains("marker=document.createElement('div')"));
        assert!(PANEL_JS.contains("classList.add('fluid--offline')"));
    }
}
