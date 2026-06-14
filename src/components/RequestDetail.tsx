import { useEffect, useState } from "react";
import type { RequestRecord } from "../types";
import { api } from "../api";
import { fmtCost, fmtMs, fmtNum, shortModel } from "../format";

interface Props {
  id: string | null;
  onClose: () => void;
}

export function RequestDetail({ id, onClose }: Props) {
  const [rec, setRec] = useState<RequestRecord | null>(null);

  useEffect(() => {
    if (!id) {
      setRec(null);
      return;
    }
    api.getRequest(id).then(setRec);
  }, [id]);

  if (!id) return null;

  let pretty = rec?.request_body ?? "";
  try {
    if (rec?.request_body) pretty = JSON.stringify(JSON.parse(rec.request_body), null, 2);
  } catch {
    /* keep raw */
  }

  return (
    <aside className="detail">
      <div className="detail-head">
        <strong>请求详情</strong>
        <button className="icon-btn" onClick={onClose}>
          ✕
        </button>
      </div>

      {!rec ? (
        <div className="muted">加载中…</div>
      ) : (
        <div className="detail-body">
          <div className="kv">
            <Row k="模型" v={shortModel(rec.model)} />
            <Row k="路径" v={`${rec.method} ${rec.path}`} />
            <Row k="状态" v={rec.error ? `ERR: ${rec.error}` : String(rec.status ?? "—")} />
            <Row k="上游" v={rec.upstream_label ?? "—"} />
            <Row k="TTFB / 时长" v={`${fmtMs(rec.ttfb_ms)} / ${fmtMs(rec.duration_ms)}`} />
            <Row
              k="Token (in/out)"
              v={`${fmtNum(rec.input_tokens)} / ${fmtNum(rec.output_tokens)}`}
            />
            <Row
              k="Cache (read/write)"
              v={`${fmtNum(rec.cache_read_tokens)} / ${fmtNum(rec.cache_creation_tokens)}`}
            />
            <Row k="成本" v={fmtCost(rec.cost_usd)} />
            <Row k="stop_reason" v={rec.stop_reason ?? "—"} />
          </div>

          {rec.response_text && (
            <>
              <h3>响应文本</h3>
              <pre className="code">{rec.response_text.slice(0, 8000)}</pre>
            </>
          )}

          <h3>请求体</h3>
          <pre className="code">{pretty || "(无)"}</pre>
        </div>
      )}
    </aside>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="kv-row">
      <span className="muted">{k}</span>
      <span>{v}</span>
    </div>
  );
}
