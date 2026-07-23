use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Mutex, RwLock};
use tauri::async_runtime::JoinHandle;

use super::cancel::CancelFlag;
use super::export;
use super::measure;
use super::rlog;
use super::types::{self, EngineState, Node};

#[derive(Default)]
pub struct SessionCtrl {
    pub cancel: Option<CancelFlag>,
    pub batch: Option<JoinHandle<()>>,
}

pub async fn run_batch(
    state: Arc<RwLock<EngineState>>,
    work_dir: PathBuf,
    http: reqwest::Client,
    cancel: CancelFlag,
) -> Result<(), String> {
    let (ping_only, socks_port, count) = {
        let st = state.read();
        (st.ping_only, st.socks_port, st.target_nodes.len())
    };

    // 与 C++ g_test_start_time 对齐：记录批次开始时刻，而非导出瞬间
    let batch_started = Instant::now();
    let test_start_time = export::format_local_test_time();
    rlog::info(
        &work_dir,
        format!("批次开始 count={count} ping_only={ping_only} socks={socks_port}"),
    );

    for idx in 0..count {
        if cancel.is_cancelled() {
            rlog::warn(&work_dir, format!("批次取消于 {}/{}", idx, count));
            break;
        }
        let node = {
            let mut st = state.write();
            let n = st
                .target_nodes
                .get(idx)
                .cloned()
                .ok_or_else(|| "节点索引越界".to_string())?;
            st.current_id = n.id;
            n
        };
        let node_id = node.id;
        let remark = node.remarks.clone();
        let ptype = node.proxy_type.clone();
        rlog::info(
            &work_dir,
            format!(
                "测节点 {}/{} id={node_id} type={ptype} name={remark}",
                idx + 1,
                count
            ),
        );
        let shared = Arc::new(Mutex::new(node));

        // 不再跑后台 sync 循环：measure 内 publish_node 已即时写入 EngineState。
        // 旧 sync 会用过期快照覆盖较新采样，把「5 次」打回「1 次」。
        measure::single_test(
            &work_dir,
            &http,
            socks_port,
            Arc::clone(&shared),
            Arc::clone(&state),
            ping_only,
            &cancel,
        )
        .await;

        {
            let mut st = state.write();
            let final_node = shared.lock().clone();
            rlog::info(
                &work_dir,
                format!(
                    "节点完成 id={node_id} online={} ping={:.0}ms avg={} max={} udp={} tls={}",
                    final_node.online,
                    final_node.site_ping_ms,
                    final_node.avg_speed,
                    final_node.max_speed,
                    final_node.nat_type,
                    final_node.tls_verified
                ),
            );
            if let Some(slot) = st.target_nodes.iter_mut().find(|x| x.id == node_id) {
                *slot = final_node;
            }
            // 取消后仍保留已测完（含本轮收尾）的节点；未开测的节点不会走到这里
            st.completed_ids.insert(node_id);
        }
        if cancel.is_cancelled() {
            rlog::warn(&work_dir, format!("批次取消于节点收尾后 {}/{}", idx + 1, count));
            break;
        }
    }

    {
        let mut st = state.write();
        st.current_id = -1;
    }

    // 只导出已完成节点，避免取消后把未测节点（默认 100% 丢包）画进结果图
    let (nodes, sort, group, mihomo_ver) = {
        let st = state.read();
        let nodes: Vec<Node> = st
            .target_nodes
            .iter()
            .filter(|n| st.completed_ids.contains(&n.id))
            .cloned()
            .collect();
        (
            nodes,
            st.sort_method.clone(),
            types::resolve_test_group(&st.custom_group, &st.target_nodes),
            st.mihomo_version.clone(),
        )
    };
    if !nodes.is_empty() {
        rlog::info(&work_dir, format!("导出结果图 group={group} nodes={}", nodes.len()));
        let meta =
            export::collect_export_meta(test_start_time, batch_started.elapsed()).await;
        if let Err(e) = export::save_results(&work_dir, &nodes, &group, &sort, &mihomo_ver, &meta)
        {
            rlog::error(&work_dir, format!("导出结果失败: {e}"));
        } else {
            rlog::info(
                &work_dir,
                format!(
                    "导出完成 duration={} traffic_nodes={}",
                    meta.duration,
                    nodes.len()
                ),
            );
        }
    }
    Ok(())
}
