import { useEffect, useRef, useState } from "react";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { formatPoint, type GpsPoint } from "../gps";
import { S } from "../strings";

type Props = {
  point: GpsPoint | null;
  editable: boolean;
  onPick: (lat: number, lon: number) => void;
};

// 画像ファイルを使わないピン (Leaflet 標準のマーカー画像はバンドル時にパスが合わないため)
const pinIcon = L.divIcon({
  className: "map-pin",
  html: '<svg width="28" height="40" viewBox="0 0 28 40" aria-hidden="true"><path d="M14 1C7 1 1.5 6.5 1.5 13.5 1.5 23 14 39 14 39s12.5-16 12.5-25.5C26.5 6.5 21 1 14 1z" fill="#2f6fe4" stroke="#fff" stroke-width="2"/><circle cx="14" cy="13.5" r="4.5" fill="#fff"/></svg>',
  iconSize: [28, 40],
  iconAnchor: [14, 39],
});

/** OpenStreetMap の地図。クリックやピンのドラッグで位置を選ぶ */
export function MapView({ point, editable, onPick }: Props) {
  const el = useRef<HTMLDivElement>(null);
  const map = useRef<L.Map | null>(null);
  const marker = useRef<L.Marker | null>(null);
  const onPickRef = useRef(onPick);
  onPickRef.current = onPick;
  const editableRef = useRef(editable);
  editableRef.current = editable;
  const [offline, setOffline] = useState(false);

  // 地図は 1 回だけ作る
  useEffect(() => {
    if (!el.current) return;
    const m = L.map(el.current, { zoomControl: true, attributionControl: true, worldCopyJump: true });
    m.attributionControl.setPrefix(false);
    let loaded = 0;
    L.tileLayer("https://tile.openstreetmap.org/{z}/{x}/{y}.png", {
      maxZoom: 19,
      // 帰属表示 (OSM の利用条件)。アプリ内でリンク先に移動しないよう文字だけにする
      attribution: "© OpenStreetMap contributors",
    })
      .on("tileload", () => {
        loaded++;
        setOffline(false);
      })
      .on("tileerror", () => {
        if (loaded === 0 || !navigator.onLine) setOffline(true);
      })
      .addTo(m);
    m.on("click", (e: L.LeafletMouseEvent) => {
      if (editableRef.current) onPickRef.current(e.latlng.lat, L.Util.wrapNum(e.latlng.lng, [-180, 180], true));
    });
    map.current = m;
    // タブ切り替え直後は大きさが 0 のことがあるので、表示後に大きさを測り直す
    const ro = new ResizeObserver(() => m.invalidateSize());
    ro.observe(el.current);
    return () => {
      ro.disconnect();
      m.remove();
      map.current = null;
      marker.current = null;
    };
  }, []);

  // 位置が変わったらピンを動かす
  useEffect(() => {
    const m = map.current;
    if (!m) return;
    if (!point) {
      marker.current?.remove();
      marker.current = null;
      if (!m.getZoom()) m.setView([20, 0], 2);
      return;
    }
    const ll = L.latLng(point.lat, point.lon);
    if (!marker.current) {
      marker.current = L.marker(ll, { icon: pinIcon, draggable: editable, keyboard: false })
        .on("dragend", (e) => {
          const p = (e.target as L.Marker).getLatLng();
          onPickRef.current(p.lat, L.Util.wrapNum(p.lng, [-180, 180], true));
        })
        .addTo(m);
      m.setView(ll, Math.max(m.getZoom() || 0, 15));
    } else {
      marker.current.setLatLng(ll);
      if (!m.getBounds().contains(ll)) m.panTo(ll);
    }
    if (editable) marker.current.dragging?.enable();
    else marker.current.dragging?.disable();
  }, [point?.lat, point?.lon, editable]);

  return (
    <div className="map-wrap">
      <div className="map" ref={el} />
      <div className="map-caption">
        <span>{point ? formatPoint(point) : S.mapNoPoint}</span>
        {editable && <span className="muted">{S.mapHint}</span>}
      </div>
      {offline && <div className="map-offline">{S.mapOffline}</div>}
    </div>
  );
}
