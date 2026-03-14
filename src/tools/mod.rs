pub mod breakpoint;
pub mod execution;
pub mod inspect;
pub mod launch;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};

use crate::state::AppState;

/// MCP server exposing debug tools to AI agents.
#[derive(Clone)]
pub struct DebugServer {
    pub state: AppState,
    tool_router: ToolRouter<Self>,
}

impl DebugServer {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl DebugServer {
    #[tool(name = "debug_launch", description = "Start a debugging session by launching a program under a debug adapter")]
    async fn debug_launch(
        &self,
        params: Parameters<launch::LaunchParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_launch(params.0).await
    }

    #[tool(name = "debug_attach", description = "Attach to an already-running process by PID")]
    async fn debug_attach(
        &self,
        params: Parameters<launch::AttachParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_attach(params.0).await
    }

    #[tool(name = "debug_set_breakpoint", description = "Set a breakpoint at a file and line, with optional condition")]
    async fn debug_set_breakpoint(
        &self,
        params: Parameters<breakpoint::SetBreakpointParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_set_breakpoint(params.0).await
    }

    #[tool(name = "debug_remove_breakpoint", description = "Remove a breakpoint at a file and line")]
    async fn debug_remove_breakpoint(
        &self,
        params: Parameters<breakpoint::RemoveBreakpointParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_remove_breakpoint(params.0).await
    }

    #[tool(name = "debug_continue", description = "Resume execution until the next breakpoint or process exit")]
    async fn debug_continue(
        &self,
        params: Parameters<execution::ContinueParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_continue(params.0).await
    }

    #[tool(name = "debug_step", description = "Step in, out, or over the current line")]
    async fn debug_step(
        &self,
        params: Parameters<execution::StepParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_step(params.0).await
    }

    #[tool(name = "debug_get_stack", description = "Get the current call stack with surrounding source code context")]
    async fn debug_get_stack(
        &self,
        params: Parameters<inspect::GetStackParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_get_stack(params.0).await
    }

    #[tool(name = "debug_evaluate", description = "Evaluate an expression or read a variable in the current debug frame")]
    async fn debug_evaluate(
        &self,
        params: Parameters<inspect::EvaluateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_evaluate(params.0).await
    }

    #[tool(name = "debug_pause", description = "Pause execution of one or all threads")]
    async fn debug_pause(
        &self,
        params: Parameters<execution::PauseParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_pause(params.0).await
    }

    #[tool(name = "debug_threads", description = "List all threads in the debuggee")]
    async fn debug_threads(
        &self,
        _params: Parameters<execution::ThreadsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_threads().await
    }

    #[tool(name = "debug_get_page", description = "Fetch the next page of a truncated debug result using a pagination token")]
    async fn debug_get_page(
        &self,
        params: Parameters<inspect::GetPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_get_page(params.0).await
    }

    #[tool(name = "debug_disconnect", description = "End the debug session, terminate the debuggee, and clean up")]
    async fn debug_disconnect(&self) -> Result<CallToolResult, McpError> {
        self.handle_disconnect().await
    }
}

#[tool_handler]
impl ServerHandler for DebugServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }
}
