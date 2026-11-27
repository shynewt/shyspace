//! Linking a device and managing your own, from the terminal: what to print
//! and what to ask. Everything else is in the core.

use std::{io::BufRead, time::Duration};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use shy_core::{
    DataDir, JoinCancel, Networking, Workspace,
    membership::link::Platform,
    views::DeviceInfo,
    workspace::{CheckOnNewDevice, DeviceLinking, ResumedLink},
};

pub const LINK_CODE_VARIABLE: &str = "SHY_LINK_CODE";
const WHAT_IT_GETS: &str =
    "This device will see the workspace's history and channels, not private channels or DMs yet.";
const POLL: Duration = Duration::from_secs(2);
/// How long `shy link` keeps syncing after the confirmation, for the
/// admin's app to add the device.
const ADDED_WAIT: Duration = Duration::from_secs(30);

#[derive(Subcommand)]
pub enum DevicesCommand {
    /// Ask the admin's app to remove one of your devices.
    Remove {
        /// The device's id, from `shy devices`.
        id: String,
        /// Also remove the devices it linked, and those they linked.
        #[arg(long)]
        with_linked: bool,
    },
}

/// Shows a code for a new device of yours, asks which number it shows, and
/// links it once you pick the right one.
pub async fn link(workspace: &Workspace) -> Result<()> {
    workspace.sync().await;
    let code = workspace.create_link_code().await?;
    eprintln!("{WHAT_IT_GETS}");
    eprintln!("On your new device, run `shy link-join` and paste this code (it works for 10 minutes):");
    println!("{code}");
    let shown = loop {
        if let Some(shown) = workspace.pending_link().await? {
            break shown;
        }
        tokio::time::sleep(POLL).await;
    };
    eprintln!("Which number does your new device show?");
    for (index, choice) in shown.choices.iter().enumerate() {
        eprintln!("  {}  {choice}", index + 1);
    }
    eprintln!("  0  None of these");
    eprintln!("Only continue if you're linking your own device right now.");
    let choices = shown.choices.len();
    let picking = tokio::task::spawn_blocking(move || read_pick(std::io::stdin().lock(), choices));
    // Ctrl-C at the pick stops the link, as "None of these" does, so the new
    // device is told rather than left waiting.
    let picked = tokio::select! {
        picked = picking => picked.context("reading your pick")??,
        _ = tokio::signal::ctrl_c() => stop_on_interrupt(workspace).await,
    };
    let Some(picked) = picked else {
        workspace.cancel_link().await?;
        bail!("Stopped. Nothing was shared.");
    };
    let device = workspace.confirm_link(&shown.choices[picked]).await?;
    eprintln!("Linked. Your new device can read now, and can send once your workspace adds it.");
    let deadline = tokio::time::Instant::now() + ADDED_WAIT;
    while tokio::time::Instant::now() < deadline {
        workspace.sync().await;
        if workspace.is_device_added(&device)? {
            eprintln!("Your workspace added it.");
            return Ok(());
        }
        tokio::time::sleep(POLL).await;
    }
    eprintln!("It can send once the admin's app is next online.");
    Ok(())
}

/// Sends the stop, then exits at once: the blocked read of the pick would
/// otherwise hold the process until standard input closes. A second Ctrl-C
/// exits without waiting for the stop.
async fn stop_on_interrupt(workspace: &Workspace) -> ! {
    tokio::spawn(async {
        if tokio::signal::ctrl_c().await.is_ok() {
            std::process::exit(130);
        }
    });
    match workspace.cancel_link().await {
        Ok(()) => eprintln!("Stopped. Nothing was shared."),
        Err(error) => eprintln!("Stopped, but the new device couldn't be told: {error:#}"),
    }
    std::process::exit(130)
}

/// The choice's index, or `None` for "None of these" or the end of input.
/// Anything else asks again, so a typo doesn't end the link.
fn read_pick(mut input: impl BufRead, choices: usize) -> Result<Option<usize>> {
    loop {
        let mut line = String::new();
        let read = input.read_line(&mut line).context("reading your pick")?;
        if read == 0 {
            return Ok(None);
        }
        match line.trim().parse::<usize>() {
            Ok(0) => return Ok(None),
            Ok(number) if number <= choices => return Ok(Some(number - 1)),
            _ => eprintln!("Type 1 to {choices}, or 0 for none of these."),
        }
    }
}

/// Stops the link in progress in `data`: both stores are wiped.
pub async fn cancel_link_join(data: DataDir) -> Result<()> {
    match DeviceLinking::resume(data).await? {
        Some(ResumedLink::Waiting(linking)) => linking.cancel(None).await?,
        Some(ResumedLink::Reading(linking, reading)) => linking.cancel(Some(reading)).await?,
        None => bail!("there's no link in progress on this device"),
    }
    println!("Stopped. Nothing this device was handed is kept.");
    Ok(())
}

/// Links this device to a person who already uses shyspace, with the code
/// their device shows, or picks up a link a restart interrupted.
pub async fn link_join(data: DataDir, read_code: impl FnOnce() -> Result<String>) -> Result<()> {
    let path = data.path.clone();
    let cancel = JoinCancel::new();
    let (linking, reading) = match DeviceLinking::resume(data.clone()).await? {
        Some(ResumedLink::Reading(linking, reading)) => (linking, reading),
        Some(ResumedLink::Waiting(mut linking)) => {
            let reading = linking.wait_for_handover(&cancel).await?;
            (linking, reading)
        }
        None => {
            let mut linking = DeviceLinking::begin(data, &read_code()?, this_platform()).await?;
            match linking.wait_for_check(&cancel).await? {
                CheckOnNewDevice::Code(code) => eprintln!("On your other device, pick {code}"),
                // Someone else using the code ends the wait with an error.
                CheckOnNewDevice::Confirmed | CheckOnNewDevice::SomeoneElse => {}
            }
            let reading = linking.wait_for_handover(&cancel).await?;
            (linking, reading)
        }
    };
    eprintln!("This device can read now. It can send once your workspace adds it.");
    let joined = linking
        .wait_until_added(reading, &cancel, Networking::Offline)
        .await?;
    joined.sync().await;
    println!("Linked to \"{}\". Data is in {}", joined.name(), path.display());
    Ok(())
}

fn this_platform() -> Platform {
    match std::env::consts::OS {
        "macos" => Platform::MacOs,
        "windows" => Platform::Windows,
        "android" => Platform::Android,
        "ios" => Platform::Ios,
        _ => Platform::Linux,
    }
}

pub async fn run(workspace: &Workspace, command: Option<DevicesCommand>) -> Result<()> {
    workspace.sync().await;
    match command {
        None => print_devices(&workspace.my_devices()?),
        Some(DevicesCommand::Remove { id, with_linked }) => {
            match with_linked {
                true => workspace.remove_my_device_and_its_links(&id).await?,
                false => workspace.remove_my_device(&id).await?,
            }
            workspace.sync().await;
            println!("Asked to remove it. It stops getting messages once your workspace confirms.");
        }
    }
    Ok(())
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Linux => "Linux",
        Platform::MacOs => "macOS",
        Platform::Windows => "Windows",
        Platform::Android => "Android",
        Platform::Ios => "iOS",
    }
}

fn print_devices(devices: &[DeviceInfo]) {
    for device in devices {
        let name = match (device.is_this_device, device.platform) {
            (true, _) => "This device".to_owned(),
            (false, Some(platform)) => platform_name(platform).to_owned(),
            (false, None) => "Device".to_owned(),
        };
        let state = if device.is_linking {
            "linking…"
        } else if device.is_removing {
            "removing…"
        } else if device.is_active_now {
            "active now"
        } else if device.has_no_activity_seen {
            "no activity seen yet"
        } else {
            "seen before"
        };
        let first = if device.is_first_device {
            ", first device"
        } else {
            ""
        };
        println!("{name}{first}  {state}  {}", device.id);
        if !device.linked_devices.is_empty() {
            println!("    linked: {}", device.linked_devices.join(", "));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_is_a_choice_or_none_and_anything_else_asks_again() {
        assert_eq!(read_pick(&b"2\n"[..], 3).unwrap(), Some(1));
        assert_eq!(read_pick(&b"0\n"[..], 3).unwrap(), None);
        assert_eq!(read_pick(&b"7\nx\n3\n"[..], 3).unwrap(), Some(2));
        assert_eq!(read_pick(&b""[..], 3).unwrap(), None);
    }
}
