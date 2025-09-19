use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::{collections::HashMap, fs, path::PathBuf};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ParsedManifest {
    pub default_launch: String,
    // (sco_identifier, href, parameters)
    pub scos: Vec<(String, String, Option<String>)>,
}

#[derive(Error, Debug)]
pub enum MfErr {
    #[error("imsmanifest.xml not found")]
    Missing,
    #[error("failed to parse manifest")]
    Parse,
}

pub fn extract_zip_to_dir(bytes: &[u8], out_dir: &PathBuf) -> anyhow::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let reader = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader)?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let outpath = out_dir.join(file.name());
        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
            continue;
        }
        if let Some(parent) = outpath.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut outfile = std::fs::File::create(&outpath)?;
        std::io::copy(&mut file, &mut outfile)?;
    }
    Ok(())
}

pub fn find_manifest(dir: &PathBuf) -> Result<PathBuf, MfErr> {
    for entry in WalkDir::new(dir) {
        let e = entry.map_err(|_| MfErr::Missing)?;
        if e.file_name() == "imsmanifest.xml" {
            return Ok(e.path().to_path_buf());
        }
    }
    Err(MfErr::Missing)
}

#[derive(Default, Debug, Clone)]
struct ResourceInfo {
    href: Option<String>,
    files: Vec<String>,
    scormtype: Option<String>,
}

pub fn parse_manifest(path: &PathBuf) -> Result<ParsedManifest, MfErr> {
    let xml = fs::read_to_string(path).map_err(|_| MfErr::Missing)?;
    let mut reader = Reader::from_str(&xml);
    reader.trim_text(true);

    let mut buf = Vec::new();

    // resources: resource identifier -> info
    let mut resources: HashMap<String, ResourceInfo> = HashMap::new();

    // items collected: (identifier, identifierref, parameters, org_id)
    let mut items: Vec<(String, String, Option<String>, Option<String>)> = Vec::new();

    // track current resource id to attach <file> tags
    let mut current_res_id: Option<String> = None;

    // organizations/default selection
    let mut in_organizations = false;
    let mut default_org_id: Option<String> = None;
    let mut current_org_id: Option<String> = None;

    // first item reference inside the selected default org
    let mut first_item_ref_in_default_org: Option<String> = None;
    // fallback: first item reference anywhere
    let mut first_item_ref_any: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "organizations" => {
                        in_organizations = true;
                        // read default="orgid" if present
                        default_org_id = get_attr(&e, "default");
                    }
                    "organization" => {
                        current_org_id = get_attr(&e, "identifier");
                    }
                    "item" => {
                        let identifier = get_attr(&e, "identifier");
                        let identifierref = get_attr(&e, "identifierref");
                        let parameters = get_attr(&e, "parameters");
                        if let (Some(id), Some(iref)) = (identifier, identifierref.clone()) {
                            let org_for_item = current_org_id.clone();
                            if first_item_ref_any.is_none() {
                                first_item_ref_any = Some(iref.clone());
                            }
                            // if this item is in the default org, remember it (first one wins)
                            let is_default_org = match (&default_org_id, &org_for_item) {
                                (Some(def), Some(cur)) => def == cur,
                                // if no default declared, treat first org as default
                                (None, Some(_)) => true,
                                _ => false,
                            };
                            if is_default_org && first_item_ref_in_default_org.is_none() {
                                first_item_ref_in_default_org = Some(iref.clone());
                            }
                            items.push((id, iref, parameters, org_for_item));
                        }
                    }
                    "resource" => {
                        // Handle non-empty <resource> ... </resource>
                        if let Some(id) = get_attr(&e, "identifier") {
                            let mut info = resources.remove(&id).unwrap_or_default();
                            if let Some(h) = get_attr(&e, "href") {
                                info.href = Some(h);
                            }
                            if let Some(st) = get_attr(&e, "scormtype")
                                .or_else(|| get_ns_attr(&e, "adlcp", "scormtype"))
                            {
                                info.scormtype = Some(st);
                            }
                            resources.insert(id.clone(), info);
                            current_res_id = Some(id);
                        }
                    }
                    "file" => {
                        // Some manifests use <file href="..."/>; sometimes Start+End, be permissive
                        if let (Some(res_id), Some(href)) =
                            (current_res_id.clone(), get_attr(&e, "href"))
                        {
                            resources
                                .entry(res_id)
                                .or_default()
                                .files
                                .push(href);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "resource" => {
                        // Handle <resource .../> (self-closing)
                        if let Some(id) = get_attr(&e, "identifier") {
                            let mut info = resources.remove(&id).unwrap_or_default();
                            if let Some(h) = get_attr(&e, "href") {
                                info.href = Some(h);
                            }
                            if let Some(st) = get_attr(&e, "scormtype")
                                .or_else(|| get_ns_attr(&e, "adlcp", "scormtype"))
                            {
                                info.scormtype = Some(st);
                            }
                            resources.insert(id, info);
                        }
                    }
                    "file" => {
                        // <file href="..."/> inside a <resource>
                        if let (Some(res_id), Some(href)) =
                            (current_res_id.clone(), get_attr(&e, "href"))
                        {
                            resources
                                .entry(res_id)
                                .or_default()
                                .files
                                .push(href);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let name = name.split(':').last().unwrap_or(&name);
                match name {
                    "organization" => {
                        current_org_id = None;
                    }
                    "organizations" => {
                        in_organizations = false;
                    }
                    "resource" => {
                        current_res_id = None;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return Err(MfErr::Parse),
            _ => {}
        }
        buf.clear();
    }

    // Choose the "default" <item> reference
    let chosen_item_ref = first_item_ref_in_default_org
        .or(first_item_ref_any)
        .or_else(|| {
            // As a last resort: pick the first resource with a usable href
            first_resource_href(&resources)
        })
        .ok_or(MfErr::Parse)?;

    // Resolve an href for that resource
    let default_launch = resolve_launch_href(&resources, &chosen_item_ref)
        .or_else(|| first_resource_href(&resources))
        .ok_or(MfErr::Parse)?;

    // Build the SCOs list
    let scos = items
        .into_iter()
        .filter_map(|(ident, identifierref, params, _org)| {
            resolve_launch_href(&resources, &identifierref)
                .map(|href| (ident, href, params))
        })
        .collect();

    Ok(ParsedManifest { default_launch, scos })
}

// ------------- helpers -------------

fn local_name(tag: &BytesStart<'_>) -> String {
    let full = String::from_utf8_lossy(tag.name().as_ref()).to_string();
    full.split(':').last().unwrap_or(&full).to_string()
}

fn get_attr(e: &BytesStart<'_>, key_local: &str) -> Option<String> {
    for a in e.attributes().flatten() {
        let key = std::str::from_utf8(a.key.as_ref()).unwrap_or_default();
        let key = key.split(':').last().unwrap_or(key);
        if key == key_local {
            return Some(a.unescape_value().ok()?.into_owned());
        }
    }
    None
}

// Sometimes tools put namespace on the attribute (e.g., adlcp:scormtype)
fn get_ns_attr(e: &BytesStart<'_>, ns: &str, key: &str) -> Option<String> {
    let want = format!("{}:{}", ns, key);
    for a in e.attributes().flatten() {
        let k = std::str::from_utf8(a.key.as_ref()).unwrap_or_default();
        if k == want {
            return Some(a.unescape_value().ok()?.into_owned());
        }
    }
    None
}

fn resolve_launch_href(resources: &HashMap<String, ResourceInfo>, identifierref: &str) -> Option<String> {
    let r = resources.get(identifierref)?;
    if let Some(h) = &r.href {
        return Some(h.clone());
    }
    // fallback: first <file href=...>
    r.files.first().cloned()
}

fn first_resource_href(resources: &HashMap<String, ResourceInfo>) -> Option<String> {
    for (_id, r) in resources.iter() {
        if let Some(h) = &r.href {
            return Some(h.clone());
        }
        if let Some(f) = r.files.first() {
            return Some(f.clone());
        }
    }
    None
}

