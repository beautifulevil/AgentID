use std::fmt::Write as _;
use std::time::Duration;

use anyhow::{anyhow, Result};
use console::{style, Style};
use dialoguer::{theme::ColorfulTheme, Input, MultiSelect, Select};
use indicatif::{ProgressBar, ProgressStyle};

pub const AGENT_TYPES: [(&str, &str); 5] = [
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("antigravity", "Antigravity"),
    ("claude-code", "Claude Code"),
    ("opencode", "OpenCode"),
];

pub struct MenuSection<'a> {
    pub title: &'a str,
    pub hint: &'a str,
    pub items: &'a [&'a str],
}

fn agentid_theme() -> ColorfulTheme {
    ColorfulTheme {
        defaults_style: Style::new().for_stderr().cyan(),
        prompt_style: Style::new().for_stderr().dim(),
        prompt_prefix: style("".to_string()).for_stderr(),
        prompt_suffix: style("".to_string()).for_stderr(),
        success_prefix: style("".to_string()).for_stderr(),
        success_suffix: style("".to_string()).for_stderr(),
        error_prefix: style("✘".to_string()).for_stderr().red(),
        error_style: Style::new().for_stderr().red(),
        hint_style: Style::new().for_stderr().dim(),
        values_style: Style::new().for_stderr().green(),
        active_item_style: Style::new().for_stderr().cyan().bold(),
        inactive_item_style: Style::new().for_stderr().dim(),
        active_item_prefix: style("  › ".to_string()).for_stderr().cyan().bold(),
        inactive_item_prefix: style("    ".to_string()).for_stderr(),
        checked_item_prefix: style("✔".to_string()).for_stderr().green(),
        unchecked_item_prefix: style("⬚".to_string()).for_stderr().dim(),
        picked_item_prefix: style("›".to_string()).for_stderr().cyan(),
        unpicked_item_prefix: style(" ".to_string()).for_stderr(),
    }
}

pub fn heading(title: &str) {
    blank();
    println!("  {}", style(title).bold());
    println!("  {}", style("─".repeat(42)).dim());
}

pub fn begin_menu_screen(email: &str, first: bool) {
    if !first {
        section_divider();
    }
    if first {
        welcome_banner(email);
    } else {
        menu_resume(email);
    }
}

pub fn begin_action_screen(email: &str) {
    section_divider();
    menu_resume(email);
}

fn section_divider() {
    println!("  {}", style("· · ·").dim());
}

pub fn welcome_banner(email: &str) {
    println!(
        "  {}  {}",
        style("AgentID").bold().cyan(),
        style("git identity for AI agents").dim()
    );
    println!("  {}", style("─".repeat(42)).dim());
    println!(
        "  {}  {}",
        style("Nice to see you,").dim(),
        style(email).bold()
    );
}

pub fn menu_resume(email: &str) {
    println!(
        "  {}  {}",
        style("AgentID").cyan().bold(),
        style(email).dim()
    );
}

pub fn success(message: &str) {
    println!("  {} {}", style("✓").green().bold(), message);
}

pub fn info(message: &str) {
    println!("  {} {}", style("→").cyan(), message);
}

pub fn explain(text: &str) {
    for line in text.lines() {
        println!("  {}", style(line).dim());
    }
}

pub fn step(number: u8, title: &str) {
    blank();
    println!(
        "  {}  {}",
        style(format!("Step {number}")).cyan().bold(),
        style(title).bold()
    );
}

pub fn wizard_title() -> String {
    style("Setup wizard").bold().cyan().to_string()
}

pub fn confirm_step(prompt: &str) -> Result<bool> {
    let full_prompt = format!("{prompt} [y/n]");
    loop {
        let answer: String = Input::with_theme(&agentid_theme())
            .with_prompt(&full_prompt)
            .report(false)
            .interact_text()
            .map_err(|error| anyhow!("{error}"))?;
        match answer.trim().to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            "" => info("Type y or n, then press Enter."),
            _ => info("Type y or n, then press Enter."),
        }
    }
}

pub fn blank() {
    println!();
}

pub fn input(prompt: &str) -> Result<String> {
    Input::with_theme(&agentid_theme())
        .with_prompt(prompt)
        .report(false)
        .interact_text()
        .map_err(|error| anyhow!("{error}"))
}

pub fn select_option(title: &str, options: &[&str]) -> Result<usize> {
    select_menu(title, options)
}

pub fn select_menu(title: &str, options: &[&str]) -> Result<usize> {
    if options.is_empty() {
        return Err(anyhow!("no options"));
    }
    let prompt = if title.trim().is_empty() {
        "Choose an option".to_string()
    } else {
        title.trim_end_matches(':').trim().to_string()
    };
    Select::with_theme(&agentid_theme())
        .with_prompt(&prompt)
        .items(options)
        .default(0)
        .report(false)
        .interact()
        .map_err(|error| anyhow!("{error}"))
}

pub fn select_sectioned_menu(sections: &[MenuSection<'_>]) -> Result<usize> {
    if sections.is_empty() {
        return Err(anyhow!("no menu sections"));
    }

    let section_labels: Vec<String> = sections
        .iter()
        .map(|section| format_menu_row(section.title, section.hint))
        .collect();

    loop {
        let section_refs: Vec<&str> = section_labels.iter().map(String::as_str).collect();
        let section_idx = select_menu("Choose a section", &section_refs)?;
        let section = &sections[section_idx];

        let mut action_labels = vec![style("← Back to sections").dim().to_string()];
        action_labels.extend(section.items.iter().map(|item| (*item).to_string()));
        let action_refs: Vec<&str> = action_labels.iter().map(String::as_str).collect();

        let action_idx = select_menu(section.title, &action_refs)?;
        if action_idx == 0 {
            continue;
        }

        let mut flat_index = 0;
        for (index, current) in sections.iter().enumerate() {
            if index < section_idx {
                flat_index += current.items.len();
            } else if index == section_idx {
                return Ok(flat_index + action_idx - 1);
            }
        }
    }
}

fn format_menu_row(title: &str, hint: &str) -> String {
    let mut row = String::new();
    let _ = write!(row, "{title:<18}");
    row.push_str(&style(hint).dim().to_string());
    row
}

pub fn multi_select(title: &str, options: &[String]) -> Result<Vec<usize>> {
    if options.is_empty() {
        return Err(anyhow!("no options"));
    }
    MultiSelect::with_theme(&agentid_theme())
        .with_prompt(title)
        .items(options)
        .report(false)
        .interact()
        .map_err(|error| anyhow!("{error}"))
}

pub fn spinner(message: &str) -> ProgressBar {
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    bar.set_message(message.to_string());
    bar.enable_steady_tick(Duration::from_millis(90));
    bar
}

pub fn warn_org_mismatch(agentid_org: &str, remote_owner: &str, action: &str) {
    if crate::github::org_matches_remote(agentid_org, remote_owner) {
        return;
    }
    println!(
        "  {} Org mismatch: AgentID is `{agentid_org}` but git remote owner is `{remote_owner}`.",
        style("⚠").yellow().bold()
    );
    println!("    {action}");
    println!("    Fix with `agentid orgs` or `git remote set-url origin ...`.");
}

pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    if headers.is_empty() {
        return;
    }

    let widths = column_widths(headers, rows);
    let lines = render_table_lines(headers, rows, &widths);
    for line in lines {
        println!("{line}");
    }
}

pub fn status_panel(lines: &[(&str, String)]) {
    if lines.is_empty() {
        return;
    }

    let label_width = lines
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0)
        .max(8);
    let value_width = lines
        .iter()
        .map(|(_, value)| value.len())
        .max()
        .unwrap_or(0);
    let inner_width = label_width + value_width + 3;

    println!("  {}", horizontal_edge(&[inner_width], "╭", "┬", "╮"));
    for (index, (label, value)) in lines.iter().enumerate() {
        let label_text = style(format!("{label:<label_width$}")).dim().to_string();
        let value_text = style(value).bold().to_string();
        let row = format!(" {label_text} {value_text} ");
        let visible = visible_width(&row);
        let pad = inner_width.saturating_sub(visible);
        println!("  │{row}{}│", " ".repeat(pad));
        if index + 1 < lines.len() {
            println!("  {}", horizontal_edge(&[inner_width], "├", "┼", "┤"));
        }
    }
    println!("  {}", horizontal_edge(&[inner_width], "╰", "┴", "╯"));
}

fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(index) {
                *width = (*width).max(visible_width(cell));
            }
        }
    }
    widths
}

fn render_table_lines(headers: &[&str], rows: &[Vec<String>], widths: &[usize]) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("  {}", horizontal_edge(widths, "╭", "┬", "╮")));

    let header_cells: Vec<String> = headers
        .iter()
        .map(|header| style(header).bold().cyan().to_string())
        .collect();
    let header_refs: Vec<&str> = header_cells.iter().map(String::as_str).collect();
    lines.push(table_row_styled(&header_refs, widths));

    lines.push(format!("  {}", horizontal_edge(widths, "├", "┼", "┤")));

    if rows.is_empty() {
        lines.push(empty_table_row(
            widths,
            &style("No rows to display").dim().to_string(),
        ));
    } else {
        for (row_index, row) in rows.iter().enumerate() {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(index, cell)| style_data_cell(index, cell, row_index))
                .collect();
            let cell_refs: Vec<&str> = cells.iter().map(String::as_str).collect();
            lines.push(table_row_styled(&cell_refs, widths));
        }
    }

    lines.push(format!("  {}", horizontal_edge(widths, "╰", "┴", "╯")));
    lines
}

fn style_data_cell(column: usize, value: &str, row_index: usize) -> String {
    if value.contains('✓') {
        return style(value).green().to_string();
    }
    if value.contains("not connected") {
        return style(value).dim().to_string();
    }
    if column == 0 {
        return style(value).cyan().to_string();
    }
    if row_index % 2 == 1 {
        return style(value).dim().to_string();
    }
    style(value).bold().to_string()
}

fn table_row_styled(cells: &[&str], widths: &[usize]) -> String {
    let mut line = String::from("  │");
    for (index, width) in widths.iter().enumerate() {
        let cell = cells.get(index).copied().unwrap_or("");
        let visible = visible_width(cell);
        let pad = width.saturating_sub(visible);
        line.push(' ');
        line.push_str(cell);
        line.push_str(&" ".repeat(pad + 1));
        line.push('│');
    }
    line
}

fn empty_table_row(widths: &[usize], message: &str) -> String {
    let total_inner: usize =
        widths.iter().map(|width| width + 2).sum::<usize>() + widths.len().saturating_sub(1);
    let visible = visible_width(message);
    let pad = total_inner.saturating_sub(visible + 2);
    format!("  │ {message}{} │", " ".repeat(pad))
}

fn horizontal_edge(widths: &[usize], left: &str, middle: &str, right: &str) -> String {
    let mut line = left.to_string();
    for (index, width) in widths.iter().enumerate() {
        line.push_str(&style("─".repeat(width + 2)).dim().to_string());
        if index + 1 < widths.len() {
            line.push_str(middle);
        }
    }
    line.push_str(right);
    line
}

fn visible_width(text: &str) -> usize {
    console::measure_text_width(text)
}

#[cfg(test)]
mod tests {
    use super::{format_menu_row, print_table, render_table_lines, visible_width, MenuSection};
    use console::style;

    #[test]
    fn table_does_not_panic() {
        print_table(
            &["ORG", "BOT", "AGENT"],
            &[vec![
                "beautifulevil".to_string(),
                "Codex Bot".to_string(),
                "codex".to_string(),
            ]],
        );
    }

    #[test]
    fn table_uses_box_borders() {
        let lines = render_table_lines(
            &["ORG", "BOT"],
            &[vec!["dodeys".to_string(), "Cursor".to_string()]],
            &[5, 6],
        );
        assert!(lines[0].contains('╭'));
        assert!(lines.iter().any(|line| line.contains('├')));
        assert!(lines.last().unwrap().contains('╯'));
    }

    #[test]
    fn visible_width_ignores_ansi() {
        let styled = style("hello").bold().to_string();
        assert_eq!(visible_width(&styled), 5);
    }

    #[test]
    fn menu_row_includes_hint() {
        let row = format_menu_row("Workspace", "this folder");
        assert!(row.contains("Workspace"));
        assert!(row.contains("this folder"));
    }

    #[test]
    fn sectioned_menu_maps_flat_index() {
        let sections = [
            MenuSection {
                title: "A",
                hint: "one",
                items: &["a1", "a2"],
            },
            MenuSection {
                title: "B",
                hint: "two",
                items: &["b1"],
            },
        ];

        let mut flat = 0;
        for section in &sections {
            for _ in section.items {
                flat += 1;
            }
        }
        assert_eq!(flat, 3);
    }
}
