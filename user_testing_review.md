1) when the model is streaming response, if I try to scroll up and read, it continuously scrolls down to latest text. Either add a slowdown to streaming showing or simply let scrolldown be interrupted at first user command to scroll up. 
2) altough the chat input pane has been increased, users input is still limiting to use only one row (I see 3 possible rows)
3) copy ai chat/ copy terminal commands are still not clear on how to use and may not even work
4) asking a model to execute command triggers error, withouth any details on error. the model was kind enough to explain what he tried to do :
     - "When I previously tried to proceed, I was implicitly moving toward:
         - "Let's read `/etc/aide/aide.conf'"
    That *requires* a tool call (`read_file`).
    However:
         You did not **explicitly authorize** that yet
         Your interpreter likely tried to auto-handle the response
         The response structure **did not match what the parser expected**
    My intended next steps were:
    1. Read /etc/aide/aide.conf
    2. Analyze:
    - Inclusion rules
    - Exclusion rules ...
    ...
    ..
    But because the enviroment is **extremely strict**, even *describing* that plan while drifting toward tool usage caused a mismatch.
    Conceptual fix : 
    Option A - Clean human mode I only explain and do not touch tools
    Option B - Explicit Tool Permission : You explicitly say something like : You may read /etc/aide/aide.conf now
    Then I will Make one clean tool call
    Stop
    Wait for the observation
    Continue normally

    What breaks it :
    Mixing explanation + implied tool usage
    Partial instructions
    Repeating error text expecting new behavior

5) Closing a TUI session should ask user for confirmation (Are you sure you want to exit ?). Also session should auto-save and ask user if he wants to save the session on exit. We auto-save for backup but if user does not wish to save, we delete the session, and if he chooses to save we shall ask for some name and other than that name add date/timestamp. Also closing the session should take the user back to the central hub, not back to terminal.
6) we need to somehow when building from git clone, we need to either identify git has or something, that later on we save in build and when app is started to make a quick check on git and if newer/different hash is on, should prompt the user to update (git clone + etiher continue to use install.sh for update or make another .sh)
