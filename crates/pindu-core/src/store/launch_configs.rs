use rusqlite::{OptionalExtension, named_params};

use super::{Result, Store, json_column, path_column, path_to_sql, to_json};
use crate::model::{GameId, LaunchConfig};

impl Store {
    /// Stores the launch config of a game, replacing any earlier one.
    pub fn set_launch_config(&self, game_id: &GameId, config: &LaunchConfig) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO launch_configs (game_id, executable, arguments, working_dir,
                environment, proton_version, prefix_path, mangohud, gamemode, wrappers)
             VALUES (:game_id, :executable, :arguments, :working_dir, :environment,
                :proton_version, :prefix_path, :mangohud, :gamemode, :wrappers)",
            named_params! {
                ":game_id": game_id,
                ":executable": path_to_sql(&config.executable)?,
                ":arguments": to_json(&config.arguments),
                ":working_dir": config.working_dir.as_deref().map(path_to_sql).transpose()?,
                ":environment": to_json(&config.environment),
                ":proton_version": config.proton_version,
                ":prefix_path": config.prefix_path.as_deref().map(path_to_sql).transpose()?,
                ":mangohud": config.mangohud,
                ":gamemode": config.gamemode,
                ":wrappers": to_json(&config.wrappers),
            },
        )?;
        Ok(())
    }

    pub fn launch_config(&self, game_id: &GameId) -> Result<Option<LaunchConfig>> {
        Ok(self
            .conn
            .query_row(
                "SELECT executable, arguments, working_dir, environment, proton_version,
                    prefix_path, mangohud, gamemode, wrappers
                 FROM launch_configs WHERE game_id = ?1",
                [game_id],
                |row| {
                    Ok(LaunchConfig {
                        executable: row.get::<_, String>("executable")?.into(),
                        arguments: json_column(row, "arguments")?,
                        working_dir: path_column(row, "working_dir")?,
                        environment: json_column(row, "environment")?,
                        proton_version: row.get("proton_version")?,
                        prefix_path: path_column(row, "prefix_path")?,
                        mangohud: row.get("mangohud")?,
                        gamemode: row.get("gamemode")?,
                        wrappers: json_column(row, "wrappers")?,
                    })
                },
            )
            .optional()?)
    }

    pub fn delete_launch_config(&self, game_id: &GameId) -> Result<()> {
        self.conn
            .execute("DELETE FROM launch_configs WHERE game_id = ?1", [game_id])?;
        Ok(())
    }
}
