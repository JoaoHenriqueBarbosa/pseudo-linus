nice-about = Run COMMAND with an adjusted niceness, which affects process scheduling.
  With no COMMAND, print the current niceness.  Niceness values range from
  -20 (most favorable to the process) to 19 (least favorable to the process).
nice-usage = nice [OPTION] [COMMAND [ARG]...]

# Error messages
# Porte pseudo-linus: as mensagens do GNU (o valor já vem citado com o `quote()` do gnulib).
nice-error-command-required-with-adjustment = a command must be given with an adjustment
nice-error-invalid-number = invalid adjustment { $value }
nice-warning-setpriority = { $util_name }: warning: setpriority: { $error }

# Help text for command-line arguments
nice-help-adjustment = add N to the niceness (default is 10)
