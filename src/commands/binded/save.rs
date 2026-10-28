use serenity::builder::{CreateAttachment, CreateMessage};
use crate::{MyErr, Components};

impl crate::reusable::postgress::account::SaveData {
    fn get_attachment(&self) -> Vec<CreateAttachment> {
        let mut x = Vec::new();
        if let Some(y) = &self.savedata { x.push(CreateAttachment::bytes(y.to_owned(), "savedata.bin")) }
        if let Some(y) = &self.partner { x.push(CreateAttachment::bytes(y.to_owned(), "partner.bin")) }
        if let Some(y) = &self.platebox { x.push(CreateAttachment::bytes(y.to_owned(), "platebox.bin")) }
        if let Some(y) = &self.skin_hist { x.push(CreateAttachment::bytes(y.to_owned(), "skin_hist.bin")) }
        if let Some(y) = &self.platedata { x.push(CreateAttachment::bytes(y.to_owned(), "platedata.bin")) }
        if let Some(y) = &self.otomoairou { x.push(CreateAttachment::bytes(y.to_owned(), "otomoairou.bin")) }
        if let Some(y) = &self.decomyset { x.push(CreateAttachment::bytes(y.to_owned(), "decomyset.bin")) }
        if let Some(y) = &self.hunternavi { x.push(CreateAttachment::bytes(y.to_owned(), "hunternavi.bin")) }
        if let Some(y) = &self.platemyset { x.push(CreateAttachment::bytes(y.to_owned(), "platemyset.bin")) }
        if let Some(y) = &self.rengokudata { x.push(CreateAttachment::bytes(y.to_owned(), "rengokudata.bin")) }
        if let Some(y) = &self.savemercenary { x.push(CreateAttachment::bytes(y.to_owned(), "savemercenary.bin")) }
        x
    }
}

#[hertz::hertz_combine_reg(300, false)]
pub async fn all<T: crate::Mybundle>(bnd: &T, reg: &crate::Reg<'_>) -> Result<(), MyErr> {
    let data = reg.pg.send_save(reg.cid).await?;
    Components::response(bnd, "🔄 **Dispatching save files to your DMs...**", true).await?;

    let user = bnd.user();
    let attachments = data.get_attachment();

    if attachments.is_empty() {
        Components::response(bnd, "⚠️ **Data not found:** You do not have any save data in the database yet.", true).await?;
        return Ok(());
    }

    for i in attachments {
        if user.id.direct_message(&bnd.ctx().http, CreateMessage::new().add_file(i)).await.is_err() {
            let instructions = "❌ DM Transmission Failed!\nThe bot cannot send your Save Data files because your Discord privacy settings are blocking Direct Messages (DMs).\nHow to fix this:\nRight-click the Rain Server icon in your server list.\nSelect Privacy Settings.\nToggle Allow direct messages from server members to ON.\nOnce enabled, please press the DM Save button again.";
            Components::response(bnd, instructions, true).await.ok();
            return Ok(());
        }
    }
    Components::response(bnd, "✅ Mission Successful! All save files have landed in your DMs. Please check your private messages.", true).await?;
    Ok(())
}