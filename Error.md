The MCP protocol has a capability negotiation step. When a server starts, it tells the client what it      
  supports. If a server provides tools, it can optionally declare the listChanged capability — meaning "I'll
  notify you if my tool list changes at runtime."                                                            
                                                            
  These servers (mcp-language-server, mcp-dap) provide tools but don't declare listChanged in their          
  capability response. The gemini client notices this mismatch and warns about it, but listens for changes
  anyway "for robustness."                                                                                   
                                                            
  It's a cosmetic warning, not a real error. The tools work fine. The fix would be in the server             
  implementations themselves — they'd need to add listChanged: true to their capabilities response during
  initialization. That's an upstream change in mcp-language-server and mcp-dap.                              
                                                            
  You can safely ignore it. If it bothers you, you could file an issue on the respective repos to add the    
  capability declaration.

