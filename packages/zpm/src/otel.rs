use std::time::Duration;

use opentelemetry::{KeyValue, trace::TracerProvider as _};
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};
use tracing_subscriber::{Layer, Registry, filter::filter_fn, layer::SubscriberExt, util::SubscriberInitExt};

pub struct OtelGuard {
    provider: SdkTracerProvider,
}

impl OtelGuard {
    pub fn shutdown(self) {
        let _ = self.provider.shutdown();
    }
}

pub fn init() -> Option<OtelGuard> {
    let has_endpoint = std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some()
        || std::env::var_os("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").is_some();

    if !has_endpoint {
        return None;
    }

    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::from_env().unwrap_or(Protocol::HttpBinary))
        .with_timeout(Duration::from_secs(5))
        .build()
    {
        Ok(exporter) => exporter,
        Err(error) => {
            eprintln!("Failed to initialize OpenTelemetry exporter: {error}");
            return None;
        },
    };

    let provider = SdkTracerProvider::builder()
        .with_resource(Resource::builder()
            .with_service_name("yarnpkg")
            .with_attribute(KeyValue::new("service.version", zpm_switch::get_bin_version()))
            .build())
        .with_simple_exporter(exporter)
        .build();

    let tracer = provider.tracer("yarn");
    let otel_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(filter_fn(|metadata| metadata.target().starts_with("yarn::")));
    let subscriber = Registry::default()
        .with(otel_layer);

    if let Err(error) = subscriber.try_init() {
        eprintln!("Failed to initialize OpenTelemetry subscriber: {error}");
        let _ = provider.shutdown();
        return None;
    }

    Some(OtelGuard {
        provider,
    })
}
