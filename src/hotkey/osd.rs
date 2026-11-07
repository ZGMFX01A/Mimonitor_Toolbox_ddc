use super::types::{AdjustDirection, AdjustParam};
use std::time::{Duration, Instant};

/// OSD 快捷键微调悬浮提示与倒计时状态
#[derive(Debug, Clone, PartialEq)]
pub struct OsdNotification {
    pub label: String,
    pub current_value: u32,
    pub countdown_remaining_secs: f64,
}

/// 倒计时到期后待提交到底层的数值载荷
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustCommitPayload {
    pub param: AdjustParam,
    pub baseline_value: i32,
    pub target_value: i32,
}

/// 单次连续按键微调会话
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustSession {
    pub param: AdjustParam,
    pub baseline_value: i32,
    pub target_value: i32,
    pub last_active: Instant,
    pub countdown_duration: Duration,
}

impl AdjustSession {
    pub fn remaining_secs(&self, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(self.last_active);
        if elapsed >= self.countdown_duration {
            0.0
        } else {
            (self.countdown_duration - elapsed).as_secs_f64()
        }
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_active) >= self.countdown_duration
    }
}

/// 纯状态聚合模型：
/// - 维护已确认基线 + 目标值
/// - 重复按键累加并重置倒计时（防抖）
/// - 松手倒计时到期触发提交（避免每次按键立即写入底层硬件）
/// - 支持取消或丢弃过期会话
#[derive(Debug, Clone)]
pub struct AdjustStateAggregator {
    default_countdown: Duration,
    active_session: Option<AdjustSession>,
}

impl Default for AdjustStateAggregator {
    fn default() -> Self {
        Self::new(Duration::from_millis(800))
    }
}

impl AdjustStateAggregator {
    pub fn new(default_countdown: Duration) -> Self {
        Self {
            default_countdown,
            active_session: None,
        }
    }

    pub fn set_countdown_duration(&mut self, duration: Duration) {
        self.default_countdown = duration;
        if let Some(ref mut session) = self.active_session {
            session.countdown_duration = duration;
        }
    }

    pub fn active_session(&self) -> Option<&AdjustSession> {
        self.active_session.as_ref()
    }

    pub fn is_active(&self) -> bool {
        self.active_session.is_some()
    }

    /// 应用一次微调输入：
    /// 返回生成的 OsdNotification，以及若发生参数切换时提前结算的前一个参数提交载荷
    pub fn apply_adjust(
        &mut self,
        param: AdjustParam,
        direction: AdjustDirection,
        step: u32,
        current_baseline: i32,
        now: Instant,
    ) -> (OsdNotification, Option<AdjustCommitPayload>) {
        let mut previous_commit = None;

        // 若当前有其他参数的会话正在进行，先自动提交前一个参数
        if let Some(session) = self.active_session.take() {
            if session.param != param {
                if session.target_value != session.baseline_value {
                    previous_commit = Some(AdjustCommitPayload {
                        param: session.param,
                        baseline_value: session.baseline_value,
                        target_value: session.target_value,
                    });
                }
            } else if !session.is_expired(now) {
                // 同一参数且未过期，恢复已有会话
                self.active_session = Some(session);
            }
        }

        let delta = match direction {
            AdjustDirection::Increase => step as i32,
            AdjustDirection::Decrease => -(step as i32),
        };

        let min_val = param.min();
        let max_val = param.max();

        let (target, baseline) = match self.active_session.take() {
            Some(mut session) => {
                // 重复按键累加并 clamp 到参数合法范围
                session.target_value = (session.target_value + delta).clamp(min_val, max_val);
                session.last_active = now;
                let t = session.target_value;
                let b = session.baseline_value;
                self.active_session = Some(session);
                (t, b)
            }
            None => {
                // 新建会话
                let base = current_baseline.clamp(min_val, max_val);
                let target = (base + delta).clamp(min_val, max_val);
                let session = AdjustSession {
                    param,
                    baseline_value: base,
                    target_value: target,
                    last_active: now,
                    countdown_duration: self.default_countdown,
                };
                self.active_session = Some(session);
                (target, base)
            }
        };

        let _ = baseline;
        let notification = OsdNotification {
            label: param.label().to_string(),
            current_value: target as u32,
            countdown_remaining_secs: self.default_countdown.as_secs_f64(),
        };

        (notification, previous_commit)
    }

    /// 轮询或倒计时到达时检查是否可以提交
    pub fn poll_commit(&mut self, now: Instant) -> Option<AdjustCommitPayload> {
        let is_ready = self
            .active_session
            .as_ref()
            .map(|s| s.is_expired(now))
            .unwrap_or(false);

        if is_ready {
            let session = self.active_session.take()?;
            if session.target_value != session.baseline_value {
                Some(AdjustCommitPayload {
                    param: session.param,
                    baseline_value: session.baseline_value,
                    target_value: session.target_value,
                })
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 取消当前正在调整的会话（用户按 ESC 或外部取消），直接丢弃改动
    pub fn cancel(&mut self) {
        self.active_session = None;
    }

    /// 确认基线值已由底层同步（如 ADB 写入成功后），更新基准
    pub fn confirm_baseline(&mut self, param: AdjustParam, value: i32) {
        if let Some(ref mut session) = self.active_session {
            if session.param == param {
                session.baseline_value = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adjust_aggregator_accumulation_and_clamp() {
        let mut agg = AdjustStateAggregator::new(Duration::from_millis(800));
        let start = Instant::now();

        // 初始背光 50，增加 5
        let (notif1, prev) = agg.apply_adjust(
            AdjustParam::Backlight,
            AdjustDirection::Increase,
            5,
            50,
            start,
        );
        assert!(prev.is_none());
        assert_eq!(notif1.current_value, 55);

        // 200ms 后再次增加 10
        let t2 = start + Duration::from_millis(200);
        let (notif2, _) = agg.apply_adjust(
            AdjustParam::Backlight,
            AdjustDirection::Increase,
            10,
            50,
            t2,
        );
        assert_eq!(notif2.current_value, 65);

        // 400ms 后连按多次，测试上限 clamp (背光 max 100)
        let t3 = start + Duration::from_millis(400);
        let (notif3, _) = agg.apply_adjust(
            AdjustParam::Backlight,
            AdjustDirection::Increase,
            50,
            50,
            t3,
        );
        assert_eq!(notif3.current_value, 100);

        // 倒计时未到期 (600ms 后，自 t3 起仅过了 200ms，未超 800ms)
        let t4 = start + Duration::from_millis(600);
        assert!(agg.poll_commit(t4).is_none());

        // 倒计时到期 (t3 + 800ms = start + 1200ms)
        let t5 = start + Duration::from_millis(1250);
        let commit = agg.poll_commit(t5);
        assert!(commit.is_some());
        let payload = commit.unwrap();
        assert_eq!(payload.param, AdjustParam::Backlight);
        assert_eq!(payload.baseline_value, 50);
        assert_eq!(payload.target_value, 100);

        // 已提交后应清空
        assert!(!agg.is_active());
    }

    #[test]
    fn test_adjust_aggregator_cancel() {
        let mut agg = AdjustStateAggregator::new(Duration::from_millis(800));
        let now = Instant::now();
        agg.apply_adjust(
            AdjustParam::Contrast,
            AdjustDirection::Decrease,
            10,
            50,
            now,
        );
        assert!(agg.is_active());

        // 取消
        agg.cancel();
        assert!(!agg.is_active());
        assert!(agg.poll_commit(now + Duration::from_secs(1)).is_none());
    }

    #[test]
    fn test_param_switch_commits_previous() {
        let mut agg = AdjustStateAggregator::new(Duration::from_millis(800));
        let t1 = Instant::now();
        agg.apply_adjust(AdjustParam::Backlight, AdjustDirection::Increase, 5, 50, t1);

        // 切换到对比度调节
        let t2 = t1 + Duration::from_millis(100);
        let (notif, prev_commit) =
            agg.apply_adjust(AdjustParam::Contrast, AdjustDirection::Increase, 5, 60, t2);

        assert_eq!(notif.current_value, 65);
        assert!(prev_commit.is_some());
        let payload = prev_commit.unwrap();
        assert_eq!(payload.param, AdjustParam::Backlight);
        assert_eq!(payload.target_value, 55);
    }
}
