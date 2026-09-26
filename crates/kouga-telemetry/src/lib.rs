//! Optional OTLP/HTTP protobuf integration. Binaries opt in explicitly.

use opentelemetry::KeyValue;
use opentelemetry_otlp::{Protocol, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{Resource, trace::Sampler};
use std::{collections::HashMap, env, error::Error as StdError, fmt, time::Duration};
use tracing_subscriber::{EnvFilter, Layer, prelude::*};

pub use kouga_runtime::{Health, HealthStatus};

pub mod propagation {
    use opentelemetry::{propagation::TextMapPropagator, trace::TraceContextExt};
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use std::collections::HashMap;
    use tracing::Span;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    /// Only W3C trace context is accepted; baggage and application headers are never copied.
    pub fn extract(parent: Option<&str>, state: Option<&str>, span: &Span) -> bool {
        remote_context(parent, state).is_some_and(|context| span.set_parent(context).is_ok())
    }

    /// Record a link to the enqueue span; each worker attempt remains a separate root span.
    pub fn link(parent: Option<&str>, state: Option<&str>, span: &Span) -> bool {
        let Some(context) = remote_context(parent, state) else {
            return false;
        };
        span.add_link(context.span().span_context().clone());
        true
    }

    fn remote_context(parent: Option<&str>, state: Option<&str>) -> Option<opentelemetry::Context> {
        let parent = parent.filter(|value| value.len() <= 55 && value.is_ascii())?;
        let mut carrier = HashMap::from([("traceparent".to_owned(), parent.to_owned())]);
        if let Some(state) = state.filter(|value| value.len() <= 512 && value.is_ascii()) {
            carrier.insert("tracestate".to_owned(), state.to_owned());
        }
        let context = TraceContextPropagator::new().extract(&carrier);
        context.span().span_context().is_valid().then_some(context)
    }

    /// Capture from the currently instrumented async task, without thread-local enter guards.
    pub fn capture() -> (Option<String>, Option<String>) {
        let context = Span::current().context();
        let mut carrier = HashMap::new();
        TraceContextPropagator::new().inject_context(&context, &mut carrier);
        (
            carrier.remove("traceparent"),
            carrier
                .remove("tracestate")
                .filter(|state| !state.is_empty()),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sampling {
    AlwaysOn,
    AlwaysOff,
    ParentBasedTraceIdRatio(f64),
}

#[derive(Clone)]
pub struct TelemetryConfig {
    pub service_name: String,
    pub service_version: Option<String>,
    pub environment: Option<String>,
    pub endpoint: Option<String>,
    pub headers: HashMap<String, String>,
    pub traces: bool,
    pub metrics: bool,
    pub logs: bool,
    pub sampling: Sampling,
    pub log_filter: String,
    pub export_timeout: Duration,
}

impl fmt::Debug for TelemetryConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryConfig")
            .field("service_name", &self.service_name)
            .field("endpoint", &self.endpoint.as_ref().map(|_| "[REDACTED]"))
            .field("headers", &"[REDACTED]")
            .field("traces", &self.traces)
            .field("metrics", &self.metrics)
            .field("logs", &self.logs)
            .finish()
    }
}

impl TelemetryConfig {
    pub fn from_env() -> Result<Self, TelemetryError> {
        let get = |key: &'static str| env::var(key).ok();
        let service_name = get("OTEL_SERVICE_NAME").unwrap_or_else(|| "kouga".into());
        let endpoint = get("OTEL_EXPORTER_OTLP_ENDPOINT");
        let enabled = |key: &'static str, default| -> Result<bool, TelemetryError> {
            match get(key).as_deref() {
                None => Ok(default),
                Some("otlp") => Ok(true),
                Some("none") => Ok(false),
                _ => Err(TelemetryError::Invalid(key)),
            }
        };
        let sampling = match get("OTEL_TRACES_SAMPLER").as_deref() {
            None | Some("always_on") => Sampling::AlwaysOn,
            Some("always_off") => Sampling::AlwaysOff,
            Some("parentbased_traceidratio") => {
                let ratio = get("OTEL_TRACES_SAMPLER_ARG")
                    .ok_or(TelemetryError::Invalid("OTEL_TRACES_SAMPLER_ARG"))?
                    .parse::<f64>()
                    .map_err(|_| TelemetryError::Invalid("OTEL_TRACES_SAMPLER_ARG"))?;
                Sampling::ParentBasedTraceIdRatio(ratio)
            }
            _ => return Err(TelemetryError::Invalid("OTEL_TRACES_SAMPLER")),
        };
        let headers = get("OTEL_EXPORTER_OTLP_HEADERS")
            .filter(|value| !value.is_empty())
            .map(|value| parse_headers(&value))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            service_name,
            service_version: get("OTEL_SERVICE_VERSION"),
            environment: get("KOUGA_ENV"),
            endpoint,
            headers,
            traces: enabled("OTEL_TRACES_EXPORTER", true)?,
            metrics: enabled("OTEL_METRICS_EXPORTER", true)?,
            logs: enabled("OTEL_LOGS_EXPORTER", false)?,
            sampling,
            log_filter: get("RUST_LOG").unwrap_or_else(|| "info".into()),
            export_timeout: Duration::from_secs(5),
        })
    }

    fn validate(&self) -> Result<(), TelemetryError> {
        if self.service_name.is_empty() || self.export_timeout.is_zero() {
            return Err(TelemetryError::Invalid("service or timeout"));
        }
        if let Sampling::ParentBasedTraceIdRatio(ratio) = self.sampling
            && (!ratio.is_finite() || !(0.0..=1.0).contains(&ratio))
        {
            return Err(TelemetryError::Invalid("OTEL_TRACES_SAMPLER_ARG"));
        }
        if let Some(endpoint) = &self.endpoint
            && !(endpoint.starts_with("http://") || endpoint.starts_with("https://"))
        {
            return Err(TelemetryError::Invalid("OTEL_EXPORTER_OTLP_ENDPOINT"));
        }
        Ok(())
    }
}

fn parse_headers(value: &str) -> Result<HashMap<String, String>, TelemetryError> {
    value
        .split(',')
        .map(|part| {
            let (name, value) = part
                .split_once('=')
                .ok_or(TelemetryError::Invalid("OTEL_EXPORTER_OTLP_HEADERS"))?;
            let name = name.trim();
            let value = percent_encoding::percent_decode_str(value.trim())
                .decode_utf8()
                .map_err(|_| TelemetryError::Invalid("OTEL_EXPORTER_OTLP_HEADERS"))?;
            if name.is_empty()
                || name
                    .bytes()
                    .any(|b| !(b.is_ascii_alphanumeric() || b == b'-'))
                || value.contains(['\r', '\n'])
            {
                return Err(TelemetryError::Invalid("OTEL_EXPORTER_OTLP_HEADERS"));
            }
            Ok((name.to_owned(), value.into_owned()))
        })
        .collect()
}

#[derive(Debug)]
pub enum TelemetryError {
    Invalid(&'static str),
    Exporter(String),
    AlreadyInitialized,
    Shutdown,
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(key) => write!(f, "invalid telemetry configuration: {key}"),
            Self::Exporter(_) => f.write_str("cannot initialize telemetry exporter"),
            Self::AlreadyInitialized => {
                f.write_str("global tracing subscriber already initialized")
            }
            Self::Shutdown => f.write_str("telemetry shutdown failed"),
        }
    }
}

impl StdError for TelemetryError {}

type ShutdownHook = Box<dyn FnOnce(Duration) -> Result<(), Box<dyn StdError + Send + Sync>> + Send>;

pub struct Telemetry {
    tracer: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    meter: Option<opentelemetry_sdk::metrics::SdkMeterProvider>,
    logger: Option<opentelemetry_sdk::logs::SdkLoggerProvider>,
    shutdown_hooks: Vec<ShutdownHook>,
}

impl Telemetry {
    /// Call once from a binary, instead of `kouga_runtime::logging::init`.
    pub fn init(config: TelemetryConfig) -> Result<Self, TelemetryError> {
        config.validate()?;
        // SDK reports the first queue drop immediately and the total on shutdown.
        let filter = EnvFilter::try_new(format!("{},opentelemetry_sdk=warn", config.log_filter))
            .map_err(|_| TelemetryError::Invalid("RUST_LOG"))?;
        let otlp_log_filter = EnvFilter::try_new(&config.log_filter)
            .map_err(|_| TelemetryError::Invalid("RUST_LOG"))?;
        let endpoint = config.endpoint.as_deref().filter(|value| !value.is_empty());
        let mut resource = Resource::builder().with_service_name(config.service_name.clone());
        if let Some(version) = config.service_version {
            resource = resource.with_attribute(KeyValue::new("service.version", version));
        }
        if let Some(environment) = config.environment {
            resource =
                resource.with_attribute(KeyValue::new("deployment.environment.name", environment));
        }
        let resource = resource.build();
        let tracer = if config.traces {
            endpoint
                .map(|endpoint| {
                    let exporter = opentelemetry_otlp::SpanExporter::builder()
                        .with_http()
                        .with_protocol(Protocol::HttpBinary)
                        .with_endpoint(format!("{}/v1/traces", endpoint.trim_end_matches('/')))
                        .with_headers(config.headers.clone())
                        .with_timeout(config.export_timeout)
                        .build()
                        .map_err(|_| TelemetryError::Exporter("traces".into()))?;
                    let sampler = match config.sampling {
                        Sampling::AlwaysOn => Sampler::AlwaysOn,
                        Sampling::AlwaysOff => Sampler::AlwaysOff,
                        Sampling::ParentBasedTraceIdRatio(ratio) => {
                            Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(ratio)))
                        }
                    };
                    Ok::<_, TelemetryError>(
                        opentelemetry_sdk::trace::SdkTracerProvider::builder()
                            .with_sampler(sampler)
                            .with_resource(resource.clone())
                            .with_batch_exporter(exporter)
                            .build(),
                    )
                })
                .transpose()?
        } else {
            None
        };
        let meter = if config.metrics {
            endpoint
                .map(|endpoint| {
                    let exporter = opentelemetry_otlp::MetricExporter::builder()
                        .with_http()
                        .with_protocol(Protocol::HttpBinary)
                        .with_endpoint(format!("{}/v1/metrics", endpoint.trim_end_matches('/')))
                        .with_headers(config.headers.clone())
                        .with_timeout(config.export_timeout)
                        .build()
                        .map_err(|_| TelemetryError::Exporter("metrics".into()))?;
                    Ok::<_, TelemetryError>(
                        opentelemetry_sdk::metrics::SdkMeterProvider::builder()
                            .with_resource(resource.clone())
                            .with_periodic_exporter(exporter)
                            .build(),
                    )
                })
                .transpose()?
        } else {
            None
        };
        let logger = if config.logs {
            endpoint
                .map(|endpoint| {
                    let exporter = opentelemetry_otlp::LogExporter::builder()
                        .with_http()
                        .with_protocol(Protocol::HttpBinary)
                        .with_endpoint(format!("{}/v1/logs", endpoint.trim_end_matches('/')))
                        .with_headers(config.headers.clone())
                        .with_timeout(config.export_timeout)
                        .build()
                        .map_err(|_| TelemetryError::Exporter("logs".into()))?;
                    Ok::<_, TelemetryError>(
                        opentelemetry_sdk::logs::SdkLoggerProvider::builder()
                            .with_resource(resource)
                            .with_batch_exporter(exporter)
                            .build(),
                    )
                })
                .transpose()?
        } else {
            None
        };
        use opentelemetry::trace::TracerProvider;
        let tracer_layer = tracer
            .as_ref()
            .map(|provider| tracing_opentelemetry::layer().with_tracer(provider.tracer("kouga")));
        let logs_layer = logger.as_ref().map(|provider| {
            opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(provider)
                .with_filter(otlp_log_filter)
                .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                    !["opentelemetry", "reqwest", "hyper", "h2"]
                        .iter()
                        .any(|prefix| meta.target().starts_with(prefix))
                }))
        });
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().json().with_filter(filter))
            .with(tracer_layer)
            .with(logs_layer)
            .try_init()
            .map_err(|_| TelemetryError::AlreadyInitialized)?;
        if let Some(provider) = &meter {
            opentelemetry::global::set_meter_provider(provider.clone());
        }
        Ok(Self {
            tracer,
            meter,
            logger,
            shutdown_hooks: Vec::new(),
        })
    }

    /// Register a custom provider for the same bounded shutdown path.
    pub fn on_shutdown(
        &mut self,
        hook: impl FnOnce(Duration) -> Result<(), Box<dyn StdError + Send + Sync>> + Send + 'static,
    ) {
        self.shutdown_hooks.push(Box::new(hook));
    }

    /// Flush buffered signals before a short-lived invocation returns.
    /// The providers remain usable for later invocations in the same process.
    pub fn flush(
        &self,
        deadline: Duration,
    ) -> impl std::future::Future<Output = Result<(), TelemetryError>> + Send + 'static {
        let tracer = self.tracer.clone();
        let meter = self.meter.clone();
        let logger = self.logger.clone();
        async move {
            let task = tokio::task::spawn_blocking(move || {
                let mut failed = false;
                if let Some(provider) = tracer {
                    failed |= provider.force_flush().is_err();
                }
                if let Some(provider) = meter {
                    failed |= provider.force_flush().is_err();
                }
                if let Some(provider) = logger {
                    failed |= provider.force_flush().is_err();
                }
                failed
            });
            match tokio::time::timeout(deadline, task).await {
                Ok(Ok(false)) => Ok(()),
                _ => Err(TelemetryError::Shutdown),
            }
        }
    }

    pub async fn shutdown(self, deadline: Duration) -> Result<(), TelemetryError> {
        let task = tokio::task::spawn_blocking(move || {
            let mut failed = false;
            if let Some(provider) = self.tracer {
                failed |= provider.shutdown_with_timeout(deadline).is_err();
            }
            if let Some(provider) = self.meter {
                failed |= provider.shutdown_with_timeout(deadline).is_err();
            }
            if let Some(provider) = self.logger {
                failed |= provider.shutdown_with_timeout(deadline).is_err();
            }
            for hook in self.shutdown_hooks {
                failed |= hook(deadline).is_err();
            }
            failed
        });
        match tokio::time::timeout(deadline, task).await {
            Ok(Ok(false)) => Ok(()),
            _ => Err(TelemetryError::Shutdown),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_config_is_rejected_without_leaking_headers() {
        let mut config = TelemetryConfig::from_env().unwrap();
        config
            .headers
            .insert("Authorization".into(), "secret".into());
        assert!(!format!("{config:?}").contains("secret"));
        config.sampling = Sampling::ParentBasedTraceIdRatio(2.0);
        assert!(matches!(config.validate(), Err(TelemetryError::Invalid(_))));
    }

    #[test]
    fn headers_decode_values_and_reject_newlines() {
        let headers = parse_headers("Authorization=Bearer%20token").unwrap();
        assert_eq!(headers["Authorization"], "Bearer token");
        assert!(parse_headers("Authorization=Bearer%0Atoken").is_err());
    }
}
