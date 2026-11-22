use std::collections::BTreeMap;
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use aurorix_core::Target;
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use thiserror::Error;

use crate::error::PluginError;
use crate::reply::Outgoing;

pub const JOB_MODULE: &str = "job";

const CRON_SEARCH_YEARS: i64 = 5;
const FIELD_NAMES: [&str; CronExpr::FIELD_COUNT] = ["分钟", "小时", "日期", "月份", "星期"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    Interval,
    Cron,
}

impl JobKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            JobKind::Interval => "interval",
            JobKind::Cron => "cron",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobSchedule {
    Interval(Duration),
    Cron(String),
}

impl JobSchedule {
    pub fn kind(&self) -> JobKind {
        match self {
            JobSchedule::Interval(_) => JobKind::Interval,
            JobSchedule::Cron(_) => JobKind::Cron,
        }
    }

    pub fn interval(&self) -> Option<Duration> {
        match self {
            JobSchedule::Interval(every) => Some(*every),
            JobSchedule::Cron(_) => None,
        }
    }

    pub fn cron(&self) -> Option<&str> {
        match self {
            JobSchedule::Interval(_) => None,
            JobSchedule::Cron(expr) => Some(expr.as_str()),
        }
    }
}

pub struct CronExpr {
    minutes: [bool; 60],
    hours: [bool; 24],
    days: [bool; 32],
    months: [bool; 13],
    weekdays: [bool; 7],
    day_restricted: bool,
    weekday_restricted: bool,
}

impl std::fmt::Debug for CronExpr {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CronExpr")
            .field("minutes", &collect_values(&self.minutes))
            .field("hours", &collect_values(&self.hours))
            .field("days", &collect_values(&self.days))
            .field("months", &collect_values(&self.months))
            .field("weekdays", &collect_values(&self.weekdays))
            .field("day_restricted", &self.day_restricted)
            .field("weekday_restricted", &self.weekday_restricted)
            .finish()
    }
}

fn collect_values(set: &[bool]) -> Vec<usize> {
    set.iter()
        .enumerate()
        .filter_map(|(value, hit)| hit.then_some(value))
        .collect()
}

impl CronExpr {
    pub const FIELD_COUNT: usize = 5;

    pub fn parse(expr: &str) -> Result<Self, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != Self::FIELD_COUNT {
            return Err(format!(
                "cron 表达式需要 {} 个字段，实际收到 {} 个",
                Self::FIELD_COUNT,
                fields.len()
            ));
        }
        let (minutes, _) = parse_field(fields[0], 0, 59, FIELD_NAMES[0], false)?;
        let (hours, _) = parse_field(fields[1], 0, 23, FIELD_NAMES[1], false)?;
        let (days, day_restricted) = parse_field(fields[2], 1, 31, FIELD_NAMES[2], true)?;
        let (months, _) = parse_field(fields[3], 1, 12, FIELD_NAMES[3], false)?;
        let (mut weekdays, weekday_restricted) =
            parse_field(fields[4], 0, 7, FIELD_NAMES[4], true)?;
        weekdays[0] = weekdays[0] || weekdays[7];
        weekdays.truncate(7);
        Ok(Self {
            minutes: to_array::<60>(minutes)?,
            hours: to_array::<24>(hours)?,
            days: to_array::<32>(days)?,
            months: to_array::<13>(months)?,
            weekdays: to_array::<7>(weekdays)?,
            day_restricted,
            weekday_restricted,
        })
    }

    pub fn next_after(&self, from: DateTime<Local>) -> Option<DateTime<Local>> {
        let base = from.naive_local().with_second(0)?.with_nanosecond(0)?;
        let mut cursor = base + chrono::Duration::minutes(1);
        let limit = base + chrono::Duration::days(CRON_SEARCH_YEARS * 366);
        while cursor <= limit {
            let date = cursor.date();
            if !self.months[date.month() as usize] {
                cursor = next_month_start(date)?;
                continue;
            }
            if !self.day_matches(date) {
                cursor = date.succ_opt()?.and_hms_opt(0, 0, 0)?;
                continue;
            }
            if !self.hours[cursor.hour() as usize] {
                cursor =
                    cursor.date().and_hms_opt(cursor.hour(), 0, 0)? + chrono::Duration::hours(1);
                continue;
            }
            if !self.minutes[cursor.minute() as usize] {
                cursor += chrono::Duration::minutes(1);
                continue;
            }
            if let Some(moment) = Local.from_local_datetime(&cursor).single() {
                return Some(moment);
            }
            cursor += chrono::Duration::minutes(1);
        }
        None
    }

    fn day_matches(&self, date: NaiveDate) -> bool {
        let day_hit = self.days[date.day() as usize];
        let weekday_hit = self.weekdays[date.weekday().num_days_from_sunday() as usize];
        match (self.day_restricted, self.weekday_restricted) {
            (true, true) => day_hit || weekday_hit,
            (true, false) => day_hit,
            (false, true) => weekday_hit,
            (false, false) => true,
        }
    }
}

fn next_month_start(date: NaiveDate) -> Option<NaiveDateTime> {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1)?.and_hms_opt(0, 0, 0)
}

fn to_array<const N: usize>(values: Vec<bool>) -> Result<[bool; N], String> {
    if values.len() != N {
        return Err(format!(
            "cron 表达式内部字段长度异常，期望 {N} 个取值，实际 {} 个",
            values.len()
        ));
    }
    let mut set = [false; N];
    set.copy_from_slice(&values);
    Ok(set)
}

fn parse_field(
    text: &str,
    min: u32,
    max: u32,
    name: &str,
    allow_question: bool,
) -> Result<(Vec<bool>, bool), String> {
    let mut set = vec![false; max as usize + 1];
    if text == "*" || (allow_question && text == "?") {
        set.fill(true);
        return Ok((set, false));
    }
    for item in text.split(',') {
        let item = item.trim();
        if item.is_empty() {
            return Err(format!("cron 表达式的{name}字段存在空项: {text}"));
        }
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => {
                let step = parse_number(step, name, 1, max, "步长")?;
                (range, step)
            }
            None => (item, 1),
        };
        let (start, end) = if range == "*" || (allow_question && range == "?") {
            (min, max)
        } else if let Some((from, to)) = range.split_once('-') {
            let from = parse_number(from, name, min, max, "起始值")?;
            let to = parse_number(to, name, min, max, "结束值")?;
            if from > to {
                return Err(format!(
                    "cron 表达式的{name}字段区间 {from}-{to} 的起始值大于结束值"
                ));
            }
            (from, to)
        } else {
            let single = parse_number(range, name, min, max, "值")?;
            if item.contains('/') {
                (single, max)
            } else {
                (single, single)
            }
        };
        let mut value = start;
        while value <= end {
            set[value as usize] = true;
            value += step;
        }
    }
    Ok((set, true))
}

fn parse_number(text: &str, name: &str, min: u32, max: u32, kind: &str) -> Result<u32, String> {
    if text.is_empty() {
        return Err(format!("cron 表达式的{name}字段缺少{kind}"));
    }
    let Ok(value) = text.parse::<u32>() else {
        return Err(format!("cron 表达式的{name}字段无法解析{kind}: {text}"));
    };
    if value < min || value > max {
        return Err(format!(
            "cron 表达式的{name}字段{kind} {value} 超出范围 {min}-{max}"
        ));
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRun {
    pub job_id: String,
    pub plugin_id: String,
    pub target: Target,
    pub fired_at: DateTime<Utc>,
}

pub type JobFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<Outgoing>, PluginError>> + Send + 'a>>;

pub trait JobHandler: Send + Sync {
    fn run<'a>(&'a self, run: JobRun) -> JobFuture<'a>;
}

struct AsyncJob<F> {
    inner: F,
}

impl<F, Fut> JobHandler for AsyncJob<F>
where
    F: Fn(JobRun) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Option<Outgoing>, PluginError>> + Send + 'static,
{
    fn run<'a>(&'a self, run: JobRun) -> JobFuture<'a> {
        Box::pin((self.inner)(run))
    }
}

struct SyncJob<F> {
    inner: F,
}

impl<F> JobHandler for SyncJob<F>
where
    F: Fn(&JobRun) -> Result<Option<Outgoing>, PluginError> + Send + Sync,
{
    fn run<'a>(&'a self, run: JobRun) -> JobFuture<'a> {
        let reply = (self.inner)(&run);
        Box::pin(ready(reply))
    }
}

pub fn async_job<F, Fut>(handler: F) -> Arc<dyn JobHandler>
where
    F: Fn(JobRun) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Option<Outgoing>, PluginError>> + Send + 'static,
{
    Arc::new(AsyncJob { inner: handler })
}

pub fn sync_job<F>(handler: F) -> Arc<dyn JobHandler>
where
    F: Fn(&JobRun) -> Result<Option<Outgoing>, PluginError> + Send + Sync + 'static,
{
    Arc::new(SyncJob { inner: handler })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    pub id: String,
    pub plugin_id: String,
    pub schedule: JobSchedule,
    pub immediate: bool,
    pub target: Target,
}

impl JobSpec {
    pub fn interval(id: impl Into<String>, plugin_id: impl Into<String>, every: Duration) -> Self {
        Self {
            id: id.into(),
            plugin_id: plugin_id.into(),
            schedule: JobSchedule::Interval(every),
            immediate: false,
            target: Target::default(),
        }
    }

    pub fn cron(
        id: impl Into<String>,
        plugin_id: impl Into<String>,
        expr: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            plugin_id: plugin_id.into(),
            schedule: JobSchedule::Cron(expr.into()),
            immediate: false,
            target: Target::default(),
        }
    }

    pub fn immediate(mut self, value: bool) -> Self {
        self.immediate = value;
        self
    }

    pub fn target(mut self, target: Target) -> Self {
        self.target = target;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobReport {
    pub id: String,
    pub plugin_id: String,
    pub kind: JobKind,
    pub cron: String,
    pub interval_ms: i64,
    pub immediate: bool,
    pub paused: bool,
    pub last_fire: i64,
    pub next_fire: i64,
    pub runs: u64,
    pub failures: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JobError {
    #[error("任务 id 不能为空")]
    MissingId,
    #[error("任务处理器不能为空")]
    MissingHandler,
    #[error("任务缺少有效的调度配置：需要正的间隔或合法的 cron 表达式")]
    MissingSchedule,
    #[error("cron 表达式不合法：{0}")]
    InvalidCron(String),
    #[error("任务 {0} 已经存在")]
    Duplicate(String),
    #[error("任务 {0} 不存在")]
    Unknown(String),
}

struct JobEntry {
    spec: JobSpec,
    handler: Arc<dyn JobHandler>,
    cron: Option<CronExpr>,
    paused: bool,
    last_fire: Option<DateTime<Utc>>,
    next_fire: Option<DateTime<Utc>>,
    runs: u64,
    failures: u64,
}

pub struct JobRegistry {
    jobs: Mutex<BTreeMap<String, JobEntry>>,
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            jobs: Mutex::new(BTreeMap::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, JobEntry>> {
        self.jobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn register(&self, spec: JobSpec, handler: Arc<dyn JobHandler>) -> Result<(), JobError> {
        if spec.id.trim().is_empty() {
            return Err(JobError::MissingId);
        }
        let anchor = Utc::now();
        let (cron, next_fire) = match &spec.schedule {
            JobSchedule::Interval(every) => {
                if every.is_zero() {
                    return Err(JobError::MissingSchedule);
                }
                let span =
                    chrono::Duration::from_std(*every).map_err(|_| JobError::MissingSchedule)?;
                let next = if spec.immediate {
                    anchor
                } else {
                    anchor + span
                };
                (None, Some(next))
            }
            JobSchedule::Cron(expr) => {
                if expr.trim().is_empty() {
                    return Err(JobError::MissingSchedule);
                }
                let parsed = CronExpr::parse(expr).map_err(JobError::InvalidCron)?;
                let next = parsed
                    .next_after(anchor.with_timezone(&Local))
                    .map(|moment| moment.with_timezone(&Utc));
                (Some(parsed), next)
            }
        };
        let entry = JobEntry {
            spec,
            handler,
            cron,
            paused: false,
            last_fire: None,
            next_fire,
            runs: 0,
            failures: 0,
        };
        self.lock().insert(entry.spec.id.clone(), entry);
        Ok(())
    }

    pub fn cancel(&self, id: &str) -> bool {
        self.lock().remove(id).is_some()
    }

    pub fn pause(&self, id: &str) -> bool {
        self.set_paused(id, true)
    }

    pub fn resume(&self, id: &str) -> bool {
        self.set_paused(id, false)
    }

    pub fn set_paused(&self, id: &str, paused: bool) -> bool {
        let mut guard = self.lock();
        match guard.get_mut(id) {
            Some(entry) => {
                entry.paused = paused;
                true
            }
            None => false,
        }
    }

    pub fn exists(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    pub fn reports(&self, now: DateTime<Utc>) -> Vec<JobReport> {
        let guard = self.lock();
        guard
            .values()
            .map(|entry| JobReport {
                id: entry.spec.id.clone(),
                plugin_id: entry.spec.plugin_id.clone(),
                kind: entry.spec.schedule.kind(),
                cron: entry.spec.schedule.cron().unwrap_or_default().to_string(),
                interval_ms: entry
                    .spec
                    .schedule
                    .interval()
                    .map(|every| every.as_millis() as i64)
                    .unwrap_or(0),
                immediate: entry.spec.immediate,
                paused: entry.paused,
                last_fire: entry.last_fire.map(timestamp_ms).unwrap_or(0),
                next_fire: next_fire_of(entry, now).map(timestamp_ms).unwrap_or(0),
                runs: entry.runs,
                failures: entry.failures,
            })
            .collect()
    }

    pub fn due(&self, now: DateTime<Utc>) -> Vec<(JobSpec, Arc<dyn JobHandler>)> {
        let mut guard = self.lock();
        let mut ready = Vec::new();
        for entry in guard.values_mut() {
            if entry.paused {
                continue;
            }
            let Some(next_fire) = entry.next_fire else {
                continue;
            };
            if next_fire > now {
                continue;
            }
            ready.push((entry.spec.clone(), Arc::clone(&entry.handler)));
            let next = advance_from(entry, now);
            entry.last_fire = Some(now);
            entry.next_fire = next;
        }
        ready
    }

    pub fn record_fire(&self, id: &str, fired_at: DateTime<Utc>) {
        let mut guard = self.lock();
        if let Some(entry) = guard.get_mut(id) {
            entry.last_fire = Some(fired_at);
            entry.runs += 1;
        }
    }

    pub fn record_failure(&self, id: &str) {
        let mut guard = self.lock();
        if let Some(entry) = guard.get_mut(id) {
            entry.failures += 1;
        }
    }
}

fn timestamp_ms(moment: DateTime<Utc>) -> i64 {
    moment.timestamp_millis()
}

fn advance_from(entry: &JobEntry, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match &entry.spec.schedule {
        JobSchedule::Interval(every) => chrono::Duration::from_std(*every)
            .ok()
            .map(|span| now + span),
        JobSchedule::Cron(_) => entry
            .cron
            .as_ref()
            .and_then(|expr| expr.next_after(now.with_timezone(&Local)))
            .map(|moment| moment.with_timezone(&Utc)),
    }
}

fn next_fire_of(entry: &JobEntry, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match &entry.spec.schedule {
        JobSchedule::Cron(_) => entry
            .cron
            .as_ref()
            .and_then(|expr| expr.next_after(now.with_timezone(&Local)))
            .map(|moment| moment.with_timezone(&Utc))
            .or(entry.next_fire),
        JobSchedule::Interval(_) => entry.next_fire,
    }
}
