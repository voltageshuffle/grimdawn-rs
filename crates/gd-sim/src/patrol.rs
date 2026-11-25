//! Patrol point groups: the world map's instance groups (map chunk 0x11,
//! `InstanceGroupManager`) that tie proxies to patrol points.
//!
//! "Patrol Points" groups list the uids of `PatrolPoint` level objects in
//! walking order; a proxy is a member of a "ProxyPatrollers" group whose
//! child groups are "Patrol Points" groups. A proxy's monsters take the points
//! of those child groups when they are attached to their controller
//! (`ControllerMonster::PostParentAttached`);
//! `Proxy:LinkPatrolPointGroup` (exe `LuaProxy::LinkPatrolPointGroup`) edits the groups
//! at run time.

use gd_formats::map::Uid;

pub const PATROL_POINTS: &str = "Patrol Points";
pub const PROXY_PATROLLERS: &str = "ProxyPatrollers";
/// Proxies of one such group share their pool choice (`GameEngine::GetEntityUnificationIndex`).
pub const UNIFIED_ENTITIES: &str = "Unified Entities";

#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    pub kind: String,
    pub id: Option<Uid>,
    /// Entity members (uids), in order.
    pub entities: Vec<Uid>,
    /// Child groups (group ids), in order.
    pub children: Vec<Uid>,
}

/// The patrol-related `InstanceGroup`s of a map.
#[derive(Clone, Debug, Default)]
pub struct PatrolGroups {
    pub groups: Vec<Group>,
    /// The `LinkedProxyPatrolGroup %d` counter (`Proxy::linkedPatrolGroupCount`).
    linked: u32,
}

/// The instance-group member id of a proxy a script created (it has no level
/// uid; the engine gives every entity one).
pub fn story_proxy_uid(id: u32) -> Uid {
    [u32::MAX, id, 0x5859_5250, 0]
}

impl PatrolGroups {
    pub fn new(groups: Vec<Group>) -> Self {
        Self { groups, linked: 0 }
    }

    /// `InstanceGroupManager::GetGroup(name)`.
    pub fn group_by_name(&self, name: &str) -> Option<usize> {
        self.groups.iter().position(|g| g.name == name)
    }

    fn group_by_id(&self, id: Uid) -> Option<usize> {
        self.groups.iter().position(|g| g.id == Some(id))
    }

    /// `InstanceGroupManager::GetGroupsThatContain`.
    fn containing(&self, uid: Uid) -> impl Iterator<Item = usize> + '_ {
        (0..self.groups.len()).filter(move |&i| self.groups[i].entities.contains(&uid))
    }

    /// The name of the "Unified Entities" group that holds `uid` (the key of the shared pool choice).
    pub fn unified_group(&self, uid: Uid) -> Option<&str> {
        self.containing(uid)
            .map(|i| &self.groups[i])
            .find(|g| g.kind == UNIFIED_ENTITIES)
            .map(|g| g.name.as_str())
    }

    /// The patrol points a proxy's monsters take when they are attached: the
    /// "Patrol Points" child groups of the "ProxyPatrollers" groups holding
    /// the proxy (`PostParentAttached`), each `FillIdList`ed in turn.
    pub fn for_proxy(&self, proxy: Uid) -> Vec<Uid> {
        let mut out = Vec::new();
        for gi in self
            .containing(proxy)
            .filter(|&i| self.groups[i].kind == PROXY_PATROLLERS)
        {
            for c in self.groups[gi]
                .children
                .iter()
                .filter_map(|&id| self.group_by_id(id))
                .filter(|&c| self.groups[c].kind == PATROL_POINTS)
            {
                out.extend(self.groups[c].entities.iter().copied());
            }
        }
        out
    }

    /// `Proxy:LinkPatrolPointGroup(name)`: the group must exist and
    /// be of type "Patrol Points" (else nothing changes: `None`). The proxy
    /// joins a "ProxyPatrollers" group if it is in none (a new
    /// `LinkedProxyPatrolGroup %d`), and in every such group the "Patrol
    /// Points" children are removed and the named group added. Returns the
    /// named group's points, for the proxy's live monsters.
    ///
    /// The name match is exact (`InstanceGroupManager::GetGroup`): the Black Lodge's
    /// `PatrolPoint_BL_Chase01/02` and the Shattered Realm's `PatrolPoint_BR_Chaser01..03`
    /// match no map group, in the game as here.
    pub fn link(&mut self, proxy: Uid, name: &str) -> Option<Vec<Uid>> {
        let pg = self
            .group_by_name(name)
            .filter(|&g| self.groups[g].kind == PATROL_POINTS)?;
        let points = self.groups[pg].entities.clone();
        let pid = self.groups[pg].id;
        let mut holders: Vec<usize> = self
            .containing(proxy)
            .filter(|&i| self.groups[i].kind == PROXY_PATROLLERS)
            .collect();
        if holders.is_empty() {
            let n = self.linked;
            self.linked += 1;
            self.groups.push(Group {
                name: format!("LinkedProxyPatrolGroup {n}"),
                kind: PROXY_PATROLLERS.into(),
                id: Some([u32::MAX, n, 0x4B4E_494C, 0]),
                entities: vec![proxy],
                children: Vec::new(),
            });
            holders.push(self.groups.len() - 1);
        }
        for h in holders {
            let kept: Vec<Uid> = self.groups[h]
                .children
                .iter()
                .copied()
                .filter(|&c| {
                    self.group_by_id(c)
                        .is_none_or(|i| self.groups[i].kind != PATROL_POINTS)
                })
                .collect();
            self.groups[h].children = kept;
            if let Some(id) = pid {
                self.groups[h].children.push(id);
            }
        }
        Some(points)
    }
}
