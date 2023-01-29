use lsp_server::{Connection, ExtractError, Message, Request, RequestId, Response};
use lsp_types::{
    notification::{DidChangeWatchedFiles, Notification, PublishDiagnostics},
    request::{GotoDefinition, References},
    Diagnostic, DidChangeWatchedFilesParams, GotoDefinitionResponse, InitializeParams, OneOf,
    PublishDiagnosticsParams, Range, ServerCapabilities,
};
use tan::api::parse_string;
use tracing::{info, trace};
use tracing_subscriber::util::SubscriberInitExt;

fn cast<R>(req: Request) -> Result<(RequestId, R::Params), ExtractError<Request>>
where
    R: lsp_types::request::Request,
    R::Params: serde::de::DeserializeOwned,
{
    req.extract(R::METHOD)
}

fn run(connection: Connection, params: serde_json::Value) -> anyhow::Result<()> {
    let _params: InitializeParams = serde_json::from_value(params).unwrap();

    for msg in &connection.receiver {
        trace!("got msg: {:?}", msg);
        match msg {
            Message::Request(req) => {
                // eprintln!("-- {}", req.method);
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                trace!("got request: {:?}", req);
                match cast::<GotoDefinition>(req.clone()) {
                    Ok((id, params)) => {
                        eprintln!("got gotoDefinition request #{id}: {params:?}");
                        let result = Some(GotoDefinitionResponse::Array(Vec::new()));
                        let result = serde_json::to_value(&result).unwrap();
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(err @ ExtractError::JsonError { .. }) => panic!("{err:?}"),
                    Err(ExtractError::MethodMismatch(req)) => req,
                };
                match cast::<References>(req.clone()) {
                    Ok((id, params)) => {
                        eprintln!("got references request #{id}: {params:?}");
                        let result = Some(Vec::<String>::new());
                        let result = serde_json::to_value(&result).unwrap();
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(err @ ExtractError::JsonError { .. }) => panic!("{err:?}"),
                    Err(ExtractError::MethodMismatch(req)) => req,
                };
                // ...
            }
            Message::Response(resp) => {
                trace!("got response: {:?}", resp);
            }
            Message::Notification(event) => {
                trace!("got notification: {:?}", event);
                if let Ok(event) =
                    event.extract::<DidChangeWatchedFilesParams>(DidChangeWatchedFiles::METHOD)
                {
                    for change in event.changes {
                        let path = change.uri.path();
                        let input = std::fs::read_to_string(path)?;
                        let res = parse_string(&input);

                        let mut diagnostics: Vec<Diagnostic> = Vec::new();

                        if let Err(errors) = res {
                            for error in errors {
                                let start = tan::range::Position::from(error.1.start, &input);
                                let start = lsp_types::Position {
                                    line: start.line as u32,
                                    character: start.col as u32,
                                };
                                let end = tan::range::Position::from(error.1.end, &input);
                                let end = lsp_types::Position {
                                    line: end.line as u32,
                                    character: end.col as u32,
                                };

                                diagnostics.push(Diagnostic {
                                    range: Range { start, end },
                                    severity: None,
                                    code: None,
                                    code_description: None,
                                    source: None,
                                    message: error.0.to_string(),
                                    related_information: None,
                                    tags: None,
                                    data: None,
                                });
                            }
                        }

                        let pdm = PublishDiagnosticsParams {
                            uri: change.uri,
                            diagnostics,
                            version: None,
                        };

                        let notification = lsp_server::Notification {
                            method: PublishDiagnostics::METHOD.to_owned(),
                            params: serde_json::to_value(&pdm).unwrap(),
                        };

                        connection
                            .sender
                            .send(Message::Notification(notification))?;

                        continue;
                    }
                }
            }
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .finish()
        .init();

    info!("starting LSP server");

    // Create the connection using stdio as the transport kind.
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(&ServerCapabilities {
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        ..Default::default()
    })
    .unwrap();

    let initialization_params = connection.initialize(server_capabilities)?;

    // Run the server.
    run(connection, initialization_params)?;

    // Wait for the two threads to end (typically by trigger LSP Exit event).
    io_threads.join()?;

    info!("shutting down server");

    Ok(())
}
