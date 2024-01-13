use std::path::Path;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::{query, query_as, query_scalar, ConnectOptions, Execute, Pool, Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations"); // defaults to "./migrations"

#[derive(Debug, Clone)]
pub struct LocalSongsDb {
    sqlite_pool: SqlitePool,
}

pub struct ChartEntry {
    pub rowid: i64,
    pub folderid: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub title_translit: String,
    pub artist_translit: String,
    pub jacket_path: String,
    pub effector: String,
    pub illustrator: String,
    pub diff_name: String,
    pub diff_shortname: String,
    pub bpm: String,
    pub diff_index: i64,
    pub level: i64,
    pub hash: String,
    pub preview_file: Option<String>,
    pub preview_offset: i64,
    pub preview_length: i64,
    pub lwt: i64,
    pub custom_offset: i64,
}

pub struct ChallengeEntry {
    pub title: String,
    pub charts: serde_json::Value,
    pub chart_meta: String,
    pub clear_mark: String,
    pub best_score: i32,
    pub req_text: String,
    pub path: String,
    pub hash: String,
    pub level: i32,
    pub lwt: i64,
}

pub struct ScoreEntry {
    pub rowid: i64,
    pub score: i64,
    pub crit: i64,
    pub near: i64,
    pub early: i64,
    pub late: i64,
    pub combo: i64,
    pub miss: i64,
    pub gauge: f64,
    pub auto_flags: i64,
    pub replay: Option<String>,
    pub timestamp: i64,
    pub chart_hash: String,
    pub user_name: String,
    pub user_id: String,
    pub local_score: bool,
    pub window_perfect: i64,
    pub window_good: i64,
    pub window_hold: i64,
    pub window_miss: i64,
    pub window_slam: i64,
    pub gauge_type: i64,
    pub gauge_opt: i64,
    pub mirror: bool,
    pub random: bool,
}

impl LocalSongsDb {
    pub async fn new(db_path: impl AsRef<Path>) -> Result<Self, sqlx::Error> {
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Memory)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Off)
            .busy_timeout(Duration::from_secs(20))
            .disable_statement_logging();

        let res = Self {
            sqlite_pool: Pool::connect_with(options).await?,
        };
        res.migrate().await?;
        Ok(res)
    }

    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        MIGRATOR.run(&self.sqlite_pool).await
    }

    pub async fn get_songs(&self) -> std::result::Result<std::vec::Vec<ChartEntry>, sqlx::Error> {
        query_as!(
            ChartEntry,
            "SELECT 
            rowid,
            folderid,
            path,
            title,
            artist,
            title_translit,
            artist_translit,
            jacket_path,
            effector,
            illustrator,
            diff_name,
            diff_shortname,
            bpm,
            diff_index,
            level,
            hash,
            preview_file,
            preview_offset,
            preview_length,
            lwt,
            custom_offset
         FROM Charts"
        )
        .fetch_all(&self.sqlite_pool)
        .await
    }

    pub async fn get_song(&self, id: i64) -> std::result::Result<ChartEntry, sqlx::Error> {
        query_as!(
            ChartEntry,
            "SELECT 
            rowid,
            folderid,
            path,
            title,
            artist,
            title_translit,
            artist_translit,
            jacket_path,
            effector,
            illustrator,
            diff_name,
            diff_shortname,
            bpm,
            diff_index,
            level,
            hash,
            preview_file,
            preview_offset,
            preview_length,
            lwt,
            custom_offset
         FROM Charts WHERE rowid = ?",
            id
        )
        .fetch_one(&self.sqlite_pool)
        .await
    }

    pub async fn get_folder_ids_query(
        &self,
        query: &str,
    ) -> std::result::Result<Vec<i64>, sqlx::Error> {
        let base_query = "SELECT DISTINCT folderId FROM Charts";
        let mut query_builder = sqlx::query_builder::QueryBuilder::new(base_query);
        let mut binds = vec![];
        if !query.is_empty() {
            for (i, term) in query.split(' ').enumerate() {
                if i == 0 {
                    query_builder.push(" WHERE")
                } else {
                    query_builder.push(" AND")
                };

                query_builder.push(
                    " (artist LIKE ?
					 OR title LIKE ?
					 OR path LIKE ?
					 OR effector LIKE ?
					 OR artist_translit LIKE ?
					 OR title_translit LIKE ?)",
                );

                for _ in 0..6 {
                    binds.push(format!("%{term}%"));
                }
            }
        }
        let mut q = query_builder.build_query_scalar();
        for ele in binds {
            q = q.bind(ele);
        }
        q.fetch_all(&self.sqlite_pool).await
    }

    pub async fn add_score(
        &self,
        ScoreEntry {
            rowid: _,
            score,
            crit,
            near,
            early,
            late,
            combo,
            miss,
            gauge,
            auto_flags,
            replay,
            timestamp,
            chart_hash,
            user_name,
            user_id,
            local_score,
            window_perfect,
            window_good,
            window_hold,
            window_miss,
            window_slam,
            gauge_type,
            gauge_opt,
            mirror,
            random,
        }: ScoreEntry,
    ) -> std::result::Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        query!("
            INSERT INTO 
			Scores(score,crit,near,early,late,combo,miss,gauge,auto_flags,replay,timestamp,chart_hash,user_name,user_id,local_score,window_perfect,window_good,window_hold,window_miss,window_slam,gauge_type,gauge_opt,mirror,random) 
			VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", 
            score,
            crit,
            near,
            early,
            late,
            combo,
            miss,
            gauge,
            auto_flags,
            replay,
            timestamp,
            chart_hash,
            user_name,
            user_id,
            local_score,
            window_perfect,
            window_good,
            window_hold,
            window_miss,
            window_slam,
            gauge_type,
            gauge_opt,
            mirror,
            random,
        ).execute(&self.sqlite_pool).await
    }

    pub async fn get_charts_for_folder(
        &self,
        id: i64,
    ) -> std::result::Result<std::vec::Vec<ChartEntry>, sqlx::Error> {
        query_as!(
            ChartEntry,
            "SELECT 
        rowid,
        folderid,
        path,
        title,
        artist,
        title_translit,
        artist_translit,
        jacket_path,
        effector,
        illustrator,
        diff_name,
        diff_shortname,
        bpm,
        diff_index,
        level,
        hash,
        preview_file,
        preview_offset,
        preview_length,
        lwt,
        custom_offset
     FROM Charts WHERE folderid = ? ORDER BY diff_index DESC",
            id
        )
        .fetch_all(&self.sqlite_pool)
        .await
    }

    pub async fn add_chart(
        &self,
        ChartEntry {
            folderid,
            path,
            title,
            artist,
            title_translit,
            artist_translit,
            jacket_path,
            effector,
            illustrator,
            diff_name,
            diff_shortname,
            bpm,
            diff_index,
            level,
            hash,
            preview_file,
            preview_offset,
            preview_length,
            lwt,
            rowid: _,
            custom_offset: _,
        }: ChartEntry,
    ) -> std::result::Result<i64, sqlx::Error> {
        query_scalar!(
            "INSERT INTO Charts(
			folderid,path,title,artist,title_translit,artist_translit,jacket_path,effector,illustrator,
			diff_name,diff_shortname,bpm,diff_index,level,hash,preview_file,preview_offset,preview_length,lwt,custom_offset) 
			VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,0) RETURNING rowid",
            folderid,
            path,
            title,
            artist,
            title_translit,
            artist_translit,
            jacket_path,
            effector,
            illustrator,
            diff_name,
            diff_shortname,
            bpm,
            diff_index,
            level,
            hash,
            preview_file,
            preview_offset,
            preview_length,
            lwt
        )
        .fetch_one(&self.sqlite_pool)
        .await
    }
    pub async fn add_folder(
        &self,
        path: String,
        id: i32,
    ) -> std::result::Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        query!("INSERT INTO Folders(path,rowid) VALUES(?,?)", path, id)
            .execute(&self.sqlite_pool)
            .await
    }
    pub async fn add_challenge(
        &self,
        ChallengeEntry {
            title,
            charts,
            chart_meta,
            clear_mark,
            best_score,
            req_text,
            path,
            hash,
            level,
            lwt,
        }: ChallengeEntry,
    ) -> std::result::Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        query!(
            "INSERT INTO Challenges(
			title,charts,chart_meta,clear_mark,best_score,req_text,path,hash,level,lwt) 
			VALUES(?,?,?,?,?,?,?,?,?,?)",
            title,
            charts,
            chart_meta,
            clear_mark,
            best_score,
            req_text,
            path,
            hash,
            level,
            lwt,
        )
        .execute(&self.sqlite_pool)
        .await
    }
    pub async fn update_chart(
        &self,
        ChartEntry {
            folderid: _,
            path,
            title,
            artist,
            title_translit,
            artist_translit,
            jacket_path,
            effector,
            illustrator,
            diff_name,
            diff_shortname,
            bpm,
            diff_index,
            level,
            hash,
            preview_file,
            preview_offset,
            preview_length,
            lwt,
            rowid: _,
            custom_offset: _,
        }: ChartEntry,
        id: i32,
    ) -> std::result::Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        query!("UPDATE Charts SET path=?,title=?,artist=?,title_translit=?,artist_translit=?,jacket_path=?,effector=?,illustrator=?,
			diff_name=?,diff_shortname=?,bpm=?,diff_index=?,level=?,hash=?,preview_file=?,preview_offset=?,preview_length=?,lwt=? WHERE rowid=?",
            path,
            title,
            artist,
            title_translit,
            artist_translit,
            jacket_path,
            effector,
            illustrator,
            diff_name,
            diff_shortname,
            bpm,
            diff_index,
            level,
            hash,
            preview_file,
            preview_offset,
            preview_length,
            lwt,
            id

        ).execute(&self.sqlite_pool).await
    }
    pub async fn update_challenge(
        &self,
        ChallengeEntry {
            title,
            charts,
            chart_meta,
            clear_mark,
            best_score,
            req_text,
            path,
            hash,
            level,
            lwt,
        }: ChallengeEntry,
        id: i32,
    ) -> std::result::Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        query!("UPDATE Challenges SET title=?,charts=?,chart_meta=?,clear_mark=?,best_score=?,req_text=?,path=?,hash=?,level=?,lwt=? WHERE rowid=?",             title,
        charts,
        chart_meta,
        clear_mark,
        best_score,
        req_text,
        path,
        hash,
        level,
        lwt,id).execute(&self.sqlite_pool).await
    }
    pub async fn remove_chart(
        &self,
        id: i32,
    ) -> std::result::Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        query!("DELETE FROM Charts WHERE rowid=?", id)
            .execute(&self.sqlite_pool)
            .await
    }
    pub async fn remove_challenge(
        &self,
        id: i32,
    ) -> std::result::Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        query!("DELETE FROM Challenges WHERE rowid=?", id)
            .execute(&self.sqlite_pool)
            .await
    }
    pub async fn remove_folder(
        &self,
        id: i32,
    ) -> std::result::Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        query!("DELETE FROM Folders WHERE rowid=?", id)
            .execute(&self.sqlite_pool)
            .await
    }
    pub async fn get_scores_for_chart(
        &self,
        chart_hash: &str,
    ) -> std::result::Result<std::vec::Vec<ScoreEntry>, sqlx::Error> {
        query_as!(
            ScoreEntry,
            "SELECT 
        rowid,
        score,
        crit,
        near,
        early,
        late,
        combo,
        miss,
        gauge,
        auto_flags,
        replay,
        timestamp,
        chart_hash,
        user_name,
        user_id,
        local_score,
        window_perfect,
        window_good,
        window_hold,
        window_miss,
        window_slam,
        gauge_type,
        gauge_opt,
        mirror,
        random
        FROM Scores WHERE chart_hash=?",
            chart_hash
        )
        .fetch_all(&self.sqlite_pool)
        .await
    }

    pub async fn get_all_scores(
        &self,
    ) -> std::result::Result<std::vec::Vec<ScoreEntry>, sqlx::Error> {
        query_as!(
            ScoreEntry,
            "SELECT 
        rowid,
        score,
        crit,
        near,
        early,
        late,
        combo,
        miss,
        gauge,
        auto_flags,
        replay,
        timestamp,
        chart_hash,
        user_name,
        user_id,
        local_score,
        window_perfect,
        window_good,
        window_hold,
        window_miss,
        window_slam,
        gauge_type,
        gauge_opt,
        mirror,
        random
        FROM Scores",
        )
        .fetch_all(&self.sqlite_pool)
        .await
    }

    pub async fn move_scores(
        &self,
        from: &str,
        to: &str,
    ) -> std::result::Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        query!(
            "UPDATE Scores set chart_hash=? where chart_hash=?",
            to,
            from
        )
        .execute(&self.sqlite_pool)
        .await
    }

    pub async fn get_or_insert_folder(
        &self,
        folder: impl AsRef<Path>,
    ) -> std::result::Result<i64, sqlx::Error> {
        if let Some(folder) = folder.as_ref().to_str() {
            let count: i64 = sqlx::query("SELECT COUNT(*) as v FROM FOLDERS WHERE PATH=?")
                .bind(folder)
                .fetch_one(&self.sqlite_pool)
                .await?
                .try_get(0)?;

            if count > 0 {
                sqlx::query("SELECT rowid FROM FOLDERS WHERE PATH=?")
                    .bind(folder)
                    .fetch_one(&self.sqlite_pool)
                    .await?
                    .try_get(0)
            } else {
                sqlx::query("INSERT INTO FOLDERS(path) VALUES(?) RETURNING rowid")
                    .bind(folder)
                    .fetch_one(&self.sqlite_pool)
                    .await?
                    .try_get(0)
            }
        } else {
            Err(sqlx::Error::RowNotFound)
        }
    }

    pub async fn get_hash_id(&self, hash: &str) -> std::result::Result<Option<i64>, sqlx::Error> {
        query_scalar!("SELECT rowid FROM Charts WHERE hash=?", hash)
            .fetch_optional(&self.sqlite_pool)
            .await
    }
}
