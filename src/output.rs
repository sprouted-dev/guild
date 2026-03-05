use colored::Colorize;

/// Print a success message.
pub fn print_success(msg: &str) {
    println!("{} {msg}", "✓".green().bold());
}

/// Print an error message to stderr.
pub fn print_error(msg: &str) {
    eprintln!("{} {msg}", "error:".red().bold());
}

/// Print a warning message to stderr.
pub fn print_warning(msg: &str) {
    eprintln!("{} {msg}", "warning:".yellow().bold());
}

/// Print a "not yet implemented" stub message.
pub fn print_not_implemented(command: &str) {
    print_warning(&format!("'{command}' is not yet implemented"));
}

/// Print a section header.
pub fn print_header(title: &str) {
    println!("\n{}", title.bold().underline());
}

/// Print a project list entry.
pub fn print_project_entry(name: &str, path: &str, tags: &[String]) {
    let tag_str = if tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", tags.join(", ").dimmed())
    };
    println!("  {} {}{tag_str}", name.cyan(), path.dimmed());
}
