use rusqlite::{params, Connection, Result};
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::collections::{HashSet, HashMap};
use std::path::Path; 
use gtk::prelude::*;
use gtk::ListStore;
use gtk::{Window, MessageDialog, MessageType, ButtonsType};
use crate::model::music_miner::miner; 
use crate::database_tables::create_all_tables;
use crate::populate_db::{insert_types, populate_database};
use crate::config::{create_database_file, create_config_dir, create_config_file};

/// Represents a song with its title, artist, and album.
pub struct SongDetails {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub path: String,
    pub track_number: i32,
    pub year: i32,
    pub genre: String,
}

/// Fetches a list of songs from the database with their details.
///
/// Queries the database for song details (title, artist, album, path, track number, year, and genre), 
/// handling potential `NULL` values for artist and album by using "Unknown".
///
/// # Arguments
/// * `connection` - Reference to a SQLite `Connection` for database access.
///
/// # Returns
/// * `Ok(Vec<SongDetails>)` - A vector of `SongDetails` structs.
/// * `Err(rusqlite::Error)` - If the query or mapping fails.
pub fn get_songs_from_database(connection: &Connection) -> Result<Vec<SongDetails>> {
    let mut stmt = connection.prepare(
        "SELECT rolas.title, performers.name, albums.name, rolas.path, rolas.track, rolas.year, rolas.genre
        FROM rolas
        LEFT JOIN performers ON rolas.id_performer = performers.id_performer
        LEFT JOIN albums ON rolas.id_album = albums.id_album"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(SongDetails {
            title: row.get(0)?,
            artist: row.get(1).unwrap_or_else(|_| String::from("Unknown")),
            album: row.get(2).unwrap_or_else(|_| String::from("Unknown")),
            path: row.get(3)?,
            track_number: row.get(4)?,
            year: row.get(5)?,
            genre: row.get(6)?,
        })
    })?;

    let mut songs = Vec::new();
    for song in rows {
        songs.push(song?);
    }
    Ok(songs)
}

/// Retrieves song titles from the database and returns them as a set of strings.
///
/// # Arguments
///
/// * `connection` - A reference to the SQLite database connection.
///
/// # Returns
///
/// Returns a `Result` containing a `HashSet<String>` with the song titles in the database.
/// If an error occurs during the query, it returns an error.
fn get_song_titles_from_database(connection: &Connection) -> Result<HashSet<String>> {
    let mut stmt = connection.prepare("SELECT title FROM rolas")?;
    let rows = stmt.query_map([], |row| {
        let title: String = row.get(0)?;
        Ok(title)
    })?;
    
    let mut db_songs = HashSet::new();
    for row in rows {
        db_songs.insert(row?);
    }
    Ok(db_songs)
}

pub fn get_song_details(title: &str) -> Result<SongDetails, Box<dyn Error>> {
    let db_path = create_database_file()?;
    let conn = Connection::open(db_path)?;

    let mut stmt = conn.prepare("SELECT title, path, track, year, genre FROM rolas WHERE title = ?1")?;
    let mut rows = stmt.query([title])?;

    if let Some(row) = rows.next()? {
        Ok(SongDetails {
            title: row.get::<_, String>(0)?,
            artist: String::from("Unknown"),
            album: String::from("Unknown"),
            path: row.get::<_, String>(1)?,
            track_number: row.get::<_, i32>(2)?,
            year: row.get::<_, i32>(3)?,
            genre: row.get::<_, String>(4)?,
        })
    } else {
        Err("No song found with the given title".into())
    }
}

/// Populates the given `ListStore` with songs from the database.
///
/// Retrieves a list of songs from the database and populates the `ListStore` with
/// song details (title, artist, album). Clears the existing contents of the list before adding the new entries.
///
/// # Arguments
/// * `list_store` - A reference to the `ListStore` where the song data will be inserted.
///
/// # Errors
/// Prints an error to the console if the database connection fails or if songs cannot be retrieved.
pub fn populate_song_list(list_store: &ListStore) {
    let connection = match create_database_connection() {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("Failed to connect to the database: {}", err);
            return;
        }
    };

    match get_songs_from_database(&connection) {
        Ok(songs) => {
            list_store.clear();
            
            for song in songs {
                let iter = list_store.append();
                list_store.set(&iter, &[
                    (0, &song.title), 
                    (1, &song.artist), 
                    (2, &song.album)
                ]);
            }
        },
        Err(err) => {
            eprintln!("Failed to retrieve songs from the database: {}", err);
        }
    };
}

/// Cleans up songs from the database that are no longer present in the specified music directory.
///
/// This function compares the song titles in the database with the file titles in
/// the directory and deletes from the database any titles that are no longer present in the directory.
///
/// # Arguments
///
/// * `connection` - A reference to the SQLite database connection.
/// * `directory_path` - The path to the directory containing the music files.
///
/// # Returns
///
/// Returns a `Result` indicating whether the operation was successful or if an error occurred.
pub fn cleanup_missing_songs(connection: &Connection, directory_path: &Path) -> Result<()> {
    let db_songs = get_song_titles_from_database(connection)?;
    
    let mut dir_songs = HashSet::new();

    for tag_map in miner::extract(directory_path.to_str().unwrap()) {
        if let Some(title) = tag_map.get("Title") {
            dir_songs.insert(title.clone());
        }
    }
    
    for title in db_songs.difference(&dir_songs) {
        connection.execute("DELETE FROM rolas WHERE title = ?1", params![title])?;
    }
    
    Ok(())
}

/// Creates a connection to the SQLite database located at `~/.config/musicmanager/database.db`.
///
/// This function first ensures that the `database.db` file exists by calling `create_database_file()`.
/// Then, it attempts to create an SQLite connection to this database file. If successful,
/// the connection object is returned for further database operations.
///
/// # Returns
///
/// - `Ok(Connection)` if the connection to the database was established successfully.
/// - `Err(rusqlite::Error)` if there was an error creating the database file or opening the connection.
pub fn create_database_connection() -> Result<Connection> {
    let file_path = match create_database_file() {
        Ok(path) => path,
        Err(e) => return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(e))),
    };

    let connection = Connection::open(file_path)?;
    Ok(connection)
}

/// Saves the provided music directory path to a configuration file.
///
/// Writes the music directory path to a `Config.TOML` file in the configuration directory.
/// If the file or directory does not exist, it is created.
///
/// # Arguments
/// * `directory` - The path to the music directory to save.
///
/// # Returns
/// * `Ok(())` - On success.
/// * `Err(io::Error)` - If there is an issue creating the config directory or writing to the file.
pub fn save_directory_to_config(directory: &str) -> io::Result<()> {
    let config_dir = create_config_dir()?;
    let file_path = config_dir.join("Config.TOML");

    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(&file_path)?;

    writeln!(file, "music_directory = \"{}\"", directory)?;

    Ok(())
}

/// Executes the complete data pipeline, which includes extracting data
/// from the music directory, creating a configuration file, connecting to the database,
/// creating and populating tables, handling missing songs, and inserting the extracted data.
///
/// # Parameters
/// * `directory`: The path to the directory containing the music files.
///
/// # Workflow
/// 1. Extracts song data from the directory using `miner::extract`.
/// 2. Creates/verifies the `Config.TOML` file.
/// 3. Establishes a connection to the database, and if successful:
///     - Creates and populates the necessary tables in the database.
///     - Manages any missing songs.
///     - Inserts the extracted data into the database.
///
/// # Errors
/// Errors are handled and reported via `eprintln!`, displaying error messages in the console.
pub fn run_data_pipeline(directory: &str) {
    let extracted_data = miner::extract(directory);

    if let Err(e) = create_config_file() {
        eprintln!("Error creating or verifying Config.TOML: {}", e);
    }

    match create_database_connection() {
        Ok(connection) => {
            if let Err(e) = create_and_populate_tables(&connection) {
                eprintln!("Error setting up the database: {}", e);
            } else {
                handle_missing_songs(&connection, directory);
                insert_extracted_data(&connection, extracted_data);
            }
        }
        Err(e) => {
            eprintln!("Error connecting to the database: {}", e);
        }
    }
}

/// Creates all necessary tables in the database and populates them with initial data.
///
/// # Parameters
/// * `connection`: The connection to the SQLite database.
///
/// # Returns
/// * `Ok(())`: If the table creation and population were successful.
/// * `Err`: If there was an error creating the tables or inserting the initial data.
///
/// # Errors
/// Errors are propagated using `Result` and handled by the caller.
fn create_and_populate_tables(connection: &rusqlite::Connection) -> Result<(), Box<dyn std::error::Error>> {
    create_all_tables(connection)?;
    insert_types(connection)?;
    println!("Database setup successfully!");
    Ok(())
}

/// Cleans up database entries for songs that are no longer present in the music directory.
///
/// # Parameters
/// * `connection`: The connection to the SQLite database.
/// * `directory`: The path to the directory containing the music files.
///
/// # Description
/// This function checks the specified directory and compares the songs present in the database
/// with those in the directory. If a song in the database is no longer present in the directory,
/// it is removed from the database.
///
/// # Errors
/// Errors encountered during cleanup are reported using `eprintln!`, with an error message printed to the console.
fn handle_missing_songs(connection: &rusqlite::Connection, directory: &str) {
    let directory_path = Path::new(directory);
    if let Err(e) = cleanup_missing_songs(connection, directory_path) {
        eprintln!("Error cleaning up missing songs: {}", e);
    } else {
        println!("Missing songs cleaned up successfully!");
    }
}

/// Inserts the extracted data from the directory into the database.
///
/// # Parameters
/// * `connection`: The connection to the SQLite database.
/// * `extracted_data`: A vector of HashMaps containing the extracted song data, where each HashMap
/// represents a song with attributes like title, artist, album, etc.
///
/// # Description
/// Iterates over each song in `extracted_data` and inserts it into the database.
/// If an insertion fails, an error is reported using `eprintln!`.
///
/// # Errors
/// If an error occurs while inserting data into the database, an error message is printed to the console.
fn insert_extracted_data(connection: &rusqlite::Connection, extracted_data: Vec<HashMap<String, String>>) {
    for tag_map in extracted_data {
        if let Err(e) = populate_database(connection, tag_map) {
            eprintln!("Error populating database: {}", e);
        } else {
            println!("Data inserted successfully!");
        }
    }
}

/// Displays an error dialog with a specific message.
///
/// # Parameters
/// * `window`: The main window on which the dialog is shown.
/// * `message`: The message that will be displayed in the dialog.
///
/// # Description
/// Creates a modal pop-up window (`MessageDialog`) that displays an error message
/// and an "OK" button to close the dialog. This dialog blocks interaction
/// with the main window until it is closed.
///
/// # Example
/// ```rust
/// show_error_dialog(&window, "No directory provided.");
/// ```
pub fn show_error_dialog(window: &Window, message: &str) {
    let dialog = MessageDialog::new(
        Some(window),
        gtk::DialogFlags::MODAL,
        MessageType::Error,
        ButtonsType::Ok,
        message,
    );
    dialog.run();
    dialog.close();
}
